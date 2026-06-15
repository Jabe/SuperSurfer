use crate::config;
use crate::context::{Context, Opener};
use crate::logging;
use crate::platform;
use crate::routing::Router;
use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "supersurfer", about = "Cross-platform browser router with TypeScript config")]
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
    let hot_path = args.len() == 1 && crate::input_url::is_routable_input(&args[0]);
    #[cfg(target_os = "windows")]
    if !hot_path {
        platform::attach_parent_console();
    }

    if hot_path {
        return platform::handle_url_arg(&args[0]);
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

fn cmd_init(register: bool, force: bool) -> Result<()> {
    let config_path = config::config_path()?;
    if config_path.exists() && !force {
        println!("Config already exists at {}", config_path.display());
    } else {
        let path = config::write_scaffold(force)?;
        println!("Created config at {}", path.0.display());
        println!("Created types at {}", config::types_path()?.display());
        println!("Scaffolded from this machine: {}", config::scaffold::summary(&path.1));
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
    println!("Default browser registration: {}", platform::registration_status());
    #[cfg(target_os = "macos")]
    if let Some(app) = platform::app_bundle_path() {
        println!("App bundle: {}", app.display());
    }
    #[cfg(target_os = "windows")]
    if let Ok(exe) = platform::exe_path() {
        println!("Executable: {}", exe.display());
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
