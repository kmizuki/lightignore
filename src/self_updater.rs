use anyhow::Result;

pub fn update() -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");

    println!("Current version: {}", current_version);
    println!("Checking for updates...");

    let status = self_update::backends::github::Update::configure()
        .repo_owner("kmizuki")
        .repo_name("lightignore")
        .bin_name("lignore")
        .current_version(current_version)
        .show_download_progress(true)
        .no_confirm(false)
        .build()?
        .update()?;

    if status.updated() {
        println!("Updated to version: {}", status.version());
        println!("Please restart the application to use the new version.");
    } else {
        println!("Already up to date!");
    }

    Ok(())
}
