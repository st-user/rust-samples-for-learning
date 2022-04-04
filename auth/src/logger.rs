use env_logger::fmt::Color;
use std::io::Write;

pub fn init_logger() {
	env_logger::Builder::new()
		.format(|buf, record| {
			let mut level_style = buf.style();
			let log_level = record.level();
			let level_color = match log_level {
				log::Level::Trace => Color::White,
				log::Level::Debug => Color::Green,
				log::Level::Info => Color::Blue,
				log::Level::Warn => Color::Yellow,
				log::Level::Error => Color::Red,
			};
			level_style.set_color(level_color).set_bold(true);

			writeln!(
				buf,
				"{} {} ({}:{})",
				level_style.value(log_level),
				record.args(),
				record.file().unwrap_or("???"),
				record.line().unwrap_or(0)
			)
		})
		.filter(
			None,
			std::env::var("RUST_LOG")
				.unwrap_or("info".to_owned())
				.parse()
				.unwrap(),
		)
		.init();
}
