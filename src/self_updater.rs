use anyhow::{Context, Result, anyhow};
use flate2::read::GzDecoder;
use reqwest::header;
use self_update::backends::github::ReleaseList;
use self_update::{Download, self_replace, version};
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tempfile::Builder;
use xz2::read::XzDecoder;
use zip::read::ZipArchive;

const BIN_NAME: &str = "lignore";

pub fn update() -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");

    println!("Current version: {}", current_version);
    println!("Checking for updates...");

    let target = self_update::get_target();
    println!("Checking target-arch... {}", target);
    println!("Checking current version... v{}", current_version);

    let releases = ReleaseList::configure()
        .repo_owner("kmizuki")
        .repo_name("lightignore")
        .with_target(&target)
        .build()
        .context("building GitHub release query")?
        .fetch()
        .context("fetching releases from GitHub")?;

    if let Some(latest) = releases.first() {
        println!(
            "Checking latest released version... v{} ({} versions available)",
            latest.version,
            releases.len()
        );
    }

    let mut candidate_release = None;
    for release in &releases {
        if version::bump_is_greater(current_version, &release.version)? {
            candidate_release = Some(release.clone());
            break;
        }
    }

    let release = match candidate_release {
        Some(release) => release,
        None => {
            println!("Already up to date!");
            return Ok(());
        }
    };

    println!(
        "New release found! v{} --> v{}",
        current_version, release.version
    );
    let compatibility_note = if version::bump_is_compatible(current_version, &release.version)? {
        ""
    } else {
        "*NOT* "
    };
    println!("New release is {}compatible", compatibility_note);

    let asset = release
        .asset_for(&target, None)
        .ok_or_else(|| anyhow!("No release asset available for target '{}'.", target))?;

    let current_exe = env::current_exe().context("locating current executable")?;

    println!("\n{} release status:", BIN_NAME);
    println!("  * Current exe: {:?}", current_exe);
    println!("  * New exe release: {:?}", asset.name);
    println!("  * New exe download url: {:?}", asset.download_url);
    println!(
        "\nThe new release will be downloaded/extracted and the existing binary will be replaced."
    );

    if !prompt_yes_no("Do you want to continue? [Y/n] ")? {
        println!("Update aborted.");
        return Ok(());
    }

    let temp_dir = Builder::new()
        .prefix("lightignore-update")
        .tempdir()
        .context("creating temporary directory")?;
    let archive_path = temp_dir.path().join(&asset.name);

    println!("Downloading...");
    let mut archive_file =
        File::create(&archive_path).context("creating temporary archive file")?;
    let mut download = Download::from_url(&asset.download_url);
    let mut headers = header::HeaderMap::new();
    headers.insert(header::ACCEPT, "application/octet-stream".parse().unwrap());
    download.set_headers(headers);
    download.show_progress(true);
    download
        .download_to(&mut archive_file)
        .context("downloading release asset")?;
    drop(archive_file);

    println!("Extracting archive...");
    let bin_name = format!("{}{}", BIN_NAME, env::consts::EXE_SUFFIX);
    let new_exe_path = unpack_asset(&archive_path, temp_dir.path(), &bin_name)
        .context("extracting downloaded archive")?;
    make_executable(&new_exe_path)?;
    println!("Replacing binary file...");
    self_replace::self_replace(&new_exe_path).context("replacing installed binary")?;

    println!("Done");
    println!("Updated to version: {}", release.version);
    println!("Please restart the application to use the new version.");

    Ok(())
}

fn prompt_yes_no(prompt: &str) -> Result<bool> {
    print!("{}", prompt);
    io::stdout().flush().context("flushing prompt")?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("reading confirmation input")?;
    let normalized = answer.trim().to_lowercase();
    Ok(normalized.is_empty() || normalized == "y" || normalized == "yes")
}

fn unpack_asset(archive_path: &Path, work_dir: &Path, bin_name: &str) -> Result<PathBuf> {
    let file_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    if file_name.ends_with(".tar.xz") {
        let file = File::open(archive_path).context("opening .tar.xz archive")?;
        extract_tar(XzDecoder::new(file), work_dir, bin_name)
    } else if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        let file = File::open(archive_path).context("opening .tar.gz archive")?;
        extract_tar(GzDecoder::new(file), work_dir, bin_name)
    } else if file_name.ends_with(".zip") {
        let file = File::open(archive_path).context("opening .zip archive")?;
        extract_zip(file, work_dir, bin_name)
    } else {
        let dest = work_dir.join(bin_name);
        fs::copy(archive_path, &dest).context("copying binary from archive")?;
        Ok(dest)
    }
}

fn extract_tar<R: io::Read>(reader: R, work_dir: &Path, bin_name: &str) -> Result<PathBuf> {
    let mut archive = tar::Archive::new(reader);
    archive.unpack(work_dir).context("unpacking tar archive")?;
    find_binary(work_dir, bin_name)
}

fn extract_zip(file: File, work_dir: &Path, bin_name: &str) -> Result<PathBuf> {
    let mut archive = ZipArchive::new(file).context("reading zip archive")?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("reading zip entry")?;
        let Some(rel_path) = entry.enclosed_name() else {
            continue;
        };
        let out_path = work_dir.join(rel_path);

        if entry.is_dir() {
            fs::create_dir_all(&out_path).context("creating directory from zip")?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).context("preparing zip output directory")?;
        }

        let mut outfile = File::create(&out_path).context("creating zip output file")?;
        io::copy(&mut entry, &mut outfile).context("extracting zip entry")?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                fs::set_permissions(&out_path, fs::Permissions::from_mode(mode))
                    .context("setting permissions on zip entry")?;
            }
        }
    }

    find_binary(work_dir, bin_name)
}

fn find_binary(root: &Path, bin_name: &str) -> Result<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let needle = OsStr::new(bin_name);

    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).context("reading extracted directory")? {
            let entry = entry.context("reading directory entry")?;
            let path = entry.path();
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                stack.push(path);
            } else if entry.file_name() == needle {
                return Ok(path);
            }
        }
    }

    Err(anyhow!("Extracted archive does not contain `{}`", bin_name))
}

fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(path).context("reading extracted binary metadata")?;
        let mut perms = metadata.permissions();
        let current = perms.mode();
        if current & 0o111 == 0 {
            perms.set_mode(current | 0o755);
            fs::set_permissions(path, perms).context("setting executable bit")?;
        }
    }
    Ok(())
}
