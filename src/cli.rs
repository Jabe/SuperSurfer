use crate::config;
use crate::context::{Context, Opener};
use crate::logging;
use crate::platform;
use crate::routing::Router;
use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "supersurfer",
    about = "Cross-platform browser router with JavaScript config"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Scaffold config + types and optionally register as default browser
    Init {
        #[arg(long)]
        register: bool,
        #[arg(long)]
        force: bool,
    },
    /// List detected browsers, validate config, show registration status
    Doctor,
    /// Dry-run routing for a URL without opening a browser
    Test {
        url: String,
        #[arg(long)]
        opener: Option<String>,
        #[arg(long)]
        open: bool,
    },
    /// Register SuperSurfer as the default browser (macOS app / Windows exe)
    #[command(name = "register")]
    RegisterApp,
    /// Fetch signed default URL-cleaning rules update (not yet implemented)
    UpdateRules,
    /// Tail routing decision log
    Logs {
        #[arg(long, default_value_t = 50)]
        lines: usize,
    },
}

pub fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hot_path = parse_hot_path(&args);
    let fresh_bootstrap = crate::bootstrap::ensure_ready()?;
    #[cfg(target_os = "windows")]
    if hot_path.is_none() {
        platform::attach_parent_console();
    }

    if let Some((url, opener)) = hot_path {
        return platform::handle_url_arg(&url, opener);
    }

    if args.is_empty() {
        return crate::bootstrap::welcome(fresh_bootstrap);
    }

    let cli = Cli::parse();
    match cli.command {
        Commands::Init { register, force } => cmd_init(register, force),
        Commands::Doctor => cmd_doctor(),
        Commands::RegisterApp => platform::register_default_browser(),
        Commands::Test { url, opener, open } => cmd_test(&url, opener.as_deref(), open),
        Commands::UpdateRules => cmd_update_rules(),
        Commands::Logs { lines } => logging::tail_logs(lines),
    }
}

/// Detect a default-browser "open this URL" invocation: exactly one routable
/// input, optionally preceded by `--opener-{name,bundle,path}` flags that the
/// platform launcher supplies to identify the originating application. Returns
/// `None` for anything else so it falls through to clap subcommand parsing.
fn parse_hot_path(args: &[String]) -> Option<(String, Option<Opener>)> {
    let mut url: Option<String> = None;
    let mut name: Option<String> = None;
    let mut bundle_id: Option<String> = None;
    let mut path: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--opener-name" => {
                name = args.get(i + 1).cloned();
                i += 2;
            }
            "--opener-bundle" => {
                bundle_id = args.get(i + 1).cloned();
                i += 2;
            }
            "--opener-path" => {
                path = args.get(i + 1).cloned();
                i += 2;
            }
            candidate if url.is_none() && crate::input_url::is_routable_input(candidate) => {
                url = Some(candidate.to_string());
                i += 1;
            }
            _ => return None,
        }
    }

    let url = url?;
    let opener = name.map(|name| Opener {
        name,
        bundle_id,
        path,
    });
    Some((url, opener))
}

fn cmd_init(register: bool, force: bool) -> Result<()> {
    let config_path = config::config_path()?;
    if config_path.exists() && !force {
        println!("Config already exists at {}", config_path.display());
    } else {
        let path = config::write_scaffold(force)?;
        println!("Created config at {}", path.0.display());
        println!("Created types at {}", config::types_path()?.display());
        println!(
            "Scaffolded from this machine: {}",
            config::scaffold::summary(&path.1)
        );
    }

    if register {
        platform::register_default_browser()?;
    } else {
        println!("Run `supersurfer register` when ready to set as default browser.");
    }
    Ok(())
}

fn cmd_doctor() -> Result<()> {
    let config_path = config::config_path()?;
    println!("Config: {}", config_path.display());
    if !config_path.exists() {
        println!("  status: missing (run `supersurfer init`)");
    } else {
        let router = Router::with_config_path(config_path)?;
        println!("  status: loaded from {}", router.config_path().display());
        println!();
        println!("Detected browsers:");
        for browser in router.registry().list() {
            println!("  - {}", browser.id);
            if let Some(app) = &browser.app_path {
                println!("      path: {app}");
            }
            for profile in &browser.profiles {
                println!(
                    "      profile: {} ({})",
                    profile.name,
                    profile.directory.as_deref().unwrap_or("-")
                );
            }
        }
    }
    println!();
    println!(
        "Default browser registration: {}",
        platform::registration_status()
    );
    #[cfg(target_os = "macos")]
    if let Some(app) = platform::app_bundle_path() {
        println!("App bundle: {}", app.display());
    }
    #[cfg(target_os = "windows")]
    if let Ok(exe) = platform::exe_path() {
        println!("Executable: {}", exe.display());
    }
    #[cfg(target_os = "linux")]
    if let Some(desktop) = platform::desktop_file_path() {
        println!("Desktop entry: {}", desktop.display());
    }
    Ok(())
}

fn cmd_test(url: &str, opener: Option<&str>, open: bool) -> Result<()> {
    let router = Router::new()?;
    let mut context = Context::default();
    if let Some(name) = opener {
        context.opener = Some(Opener {
            name: name.to_string(),
            bundle_id: None,
            path: None,
        });
    } else if router.references_opener() {
        context.opener = platform::detect_opener();
    }

    let decision = router.route_and_launch(url, &context, !open)?;
    println!("input:    {}", decision.input_url);
    println!("cleaned:  {}", decision.cleaned_url);
    println!("browser:  {}", decision.browser);
    if let Some(profile) = &decision.profile {
        println!("profile:  {profile}");
    }
    if decision.fallback {
        println!("note:     fell back to defaultBrowser");
    }
    if open {
        println!("opened in {}", decision.browser);
    } else {
        println!("(dry run — pass --open to launch)");
    }
    Ok(())
}

fn cmd_update_rules() -> Result<()> {
    println!("`supersurfer update-rules` is not implemented yet.");
    println!("Built-in URL cleaning rules ship with the binary and update on release.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn bare_url_is_hot_path_without_opener() {
        let (url, opener) = parse_hot_path(&s(&["https://example.com"])).unwrap();
        assert_eq!(url, "https://example.com");
        assert!(opener.is_none());
    }

    #[test]
    fn opener_flags_are_parsed_before_url() {
        let (url, opener) = parse_hot_path(&s(&[
            "--opener-name",
            "Slack",
            "--opener-bundle",
            "com.tinyspeck.slackmacgap",
            "https://example.com",
        ]))
        .unwrap();
        assert_eq!(url, "https://example.com");
        let opener = opener.unwrap();
        assert_eq!(opener.name, "Slack");
        assert_eq!(
            opener.bundle_id.as_deref(),
            Some("com.tinyspeck.slackmacgap")
        );
    }

    #[test]
    fn subcommands_are_not_hot_path() {
        assert!(parse_hot_path(&s(&["doctor"])).is_none());
        assert!(parse_hot_path(&s(&["test", "https://example.com"])).is_none());
        assert!(parse_hot_path(&s(&["init", "--register"])).is_none());
    }

    #[test]
    fn empty_args_are_not_hot_path() {
        assert!(parse_hot_path(&[]).is_none());
    }
}
