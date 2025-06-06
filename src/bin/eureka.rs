#[macro_use]
extern crate clap;
extern crate pretty_env_logger;
extern crate termcolor;

use clap::ArgAction;
use std::io;

use eureka::config_manager::{ConfigManagement, ConfigManager, ConfigType};
use eureka::git::Git;
use eureka::printer::Printer;
use eureka::program_access::ProgramAccess;
use eureka::reader::Reader;
use eureka::{Eureka, EurekaOptions};
use log::error;

const ARG_CLEAR_CONFIG: &str = "clear-config";
const ARG_VIEW: &str = "view";

fn main() {
    pretty_env_logger::init();

    let cli_flags = clap::Command::new("eureka")
        .author(crate_authors!())
        .version(crate_version!())
        .about("Input and store your ideas without leaving the terminal")
        .arg(
            clap::Arg::new(ARG_CLEAR_CONFIG)
                .long(ARG_CLEAR_CONFIG)
                .action(ArgAction::SetTrue)
                .help("Clear your stored configuration"),
        )
        .arg(
            clap::Arg::new(ARG_VIEW)
                .long(ARG_VIEW)
                .short(ARG_VIEW.chars().next().unwrap())
                .action(ArgAction::SetTrue)
                .help("View ideas with your $PAGER env variable. If unset use less"),
        )
        .get_matches();

    let stdio = io::stdin();
    let input = stdio.lock();
    let output = termcolor::StandardStream::stdout(termcolor::ColorChoice::Always);

    let config = ConfigManager::default();
    let ssh_key = config.config_read(ConfigType::SshKey).unwrap_or_default();

    let mut eureka = Eureka::new(
        ConfigManager::default(),
        Printer::new(output),
        Reader::new(input),
        Git::new(&ssh_key),
        ProgramAccess::default(),
    );

    let opts = EurekaOptions {
        clear_config: cli_flags.get_flag(ARG_CLEAR_CONFIG),
        view: cli_flags.get_flag(ARG_VIEW),
    };

    match eureka.run(opts) {
        Ok(_) => {}
        Err(e) => error!("{}", e),
    }
}
