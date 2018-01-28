use clap::{Arg, App, SubCommand};

pub fn build_cli() -> App<'static, 'static> {
	App::new("TimeTracker")
		.subcommand(SubCommand::with_name("down")
		)
		.subcommand(SubCommand::with_name("projects")
		)
		.arg(Arg::with_name("config")
			.short("c")
			.long("config")
			.value_name("FILE")
			.help("Sets a custom config file")
			.takes_value(true)
		)
}

