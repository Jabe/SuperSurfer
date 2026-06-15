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
    command: Option<Commands>,

    /// URL passed when invoked as the default browser handler
    #[arg(value_name = "URL")]
    url: Option<String>,
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
    /// Fetch signed default URL-cleaning rules update (not yet implemented)
    UpdateRules,
    /// Tail routing decision log
    Logs {
        #[arg(long, default_value_t = 50)]
        lines: usize,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Init { register, force }) => cmd_init(register, force),
        Some(Commands::Doctor) => cmd_doctor(),
        Some(Commands::Test { url, opener, open }) => cmd_test(&url, opener.as_deref(), open),
        Some(Commands::UpdateRules) => cmd_update_rules(),
        Some(Commands::Logs { lines }) => logging::tail_logs(lines),
        None => {
            if let Some(url) = cli.url {
                platform::handle_url_arg(&url)
            } else {
                eprintln!("SuperSurfer — pass a URL or use a subcommand. Try `supersurfer --help`.");
                Ok(())
            }
        }
    }
}

fn cmd_init(register: bool, force: bool) -> Result<()> {
    let path = config::write_scaffold(force)?;
    println!("Created config at {}", path.display());
    println!("Created types at {}", config::types_path()?.display());
    if register {
        platform::register_default_browser()?;
        println!("Registered SuperSurfer as default browser.");
    } else {
        println!("Run `supersurfer init --register` when ready to set as default browser.");
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
    } else {
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
