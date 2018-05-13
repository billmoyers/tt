use clap::{Arg, App, SubCommand, AppSettings, Shell};

pub fn build_cli() -> App<'static, 'static> {
	App::new("tt")
		.subcommand(SubCommand::with_name("down")
		)
		.subcommand(SubCommand::with_name("projects")
		)
		.subcommand(SubCommand::with_name("status")
		)
		.subcommand(SubCommand::with_name("punchin")
			.arg(Arg::with_name("project")
				.required(true)
				.index(1)
			)
		)
		.subcommand(SubCommand::with_name("punchout")
			.arg(Arg::with_name("project")
				.required(false)
				.index(1)
			)
		)
		.arg(Arg::with_name("config")
			.short("c")
			.long("config")
			.value_name("FILE")
			.help("Sets a custom config file")
			.takes_value(true)
		)
		.subcommand(SubCommand::with_name("completions")
			.about("Generate completion scripts for your shell")
			.setting(AppSettings::ArgRequiredElseHelp)
			.arg(Arg::with_name("shell").possible_values(&Shell::variants())))
}

