extern crate rusqlite;
extern crate time;
extern crate clap;
extern crate rpassword;
extern crate hyper;
extern crate hyper_tls;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;
extern crate tokio_core;
extern crate futures;

mod cli;
mod teamwork;

use time::Timespec;
use rusqlite::Connection;

#[derive(Debug, Deserialize)]
struct Project {
	id: String,
	name: String
}

#[derive(Debug)]
struct Timeblock {
	id: String,
	project_id: String,
	start: Timespec,
	end: Option<Timespec>
}

#[derive(Debug)]
pub enum Error {
	RusqliteError(rusqlite::Error),
	IOError(std::io::Error),
	HyperError(hyper::Error)
}

impl std::convert::From<rusqlite::Error> for Error {
	fn from(e: rusqlite::Error) -> Error {
		Error::RusqliteError(e)
	}
}
impl std::convert::From<std::io::Error> for Error {
	fn from(e: std::io::Error) -> Error {
		Error::IOError(e)
	}
}
impl std::convert::From<hyper::Error> for Error {
	fn from(e: hyper::Error) -> Error {
		Error::HyperError(e)
	}
}

trait TimeTracker {
	fn projects(&self) -> Result<Vec<Project>, rusqlite::Error>;
	//fn punchin(&self, proj: Project) -> Result<Timeblock, Error>;
	//fn punchout(&self, tb: Timeblock) -> Result<Timeblock, Error>;

	fn down(&self) -> Result<(), Error>;
	fn up(&self) -> Result<(), Error>;
}

fn upgrade(conn: &Connection, vto: i32) -> Result<i32, rusqlite::Error> {
	let mut vfrom: i32 = -1;

	match conn.prepare("SELECT version FROM metadata") {
		Ok(mut stmt) => {
			stmt.query_map(&[], |row| {
				vfrom = row.get(0)
			}).unwrap().next();

			println!("metadata: found, version={}...", vfrom);
		}
		_ => {
			println!("metadata: not found...");
		}
	}

	for v in vfrom+1..vto+1 {
		println!("Updating to version {}...", v);
		match v {
			0 => {
				conn.execute("
					CREATE TABLE metadata (
						version INT NOT NULL DEFAULT 0,
						teamwork_api_key TEXT,
						teamwork_base_url TEXT,
						CHECK (
							(teamwork_api_key IS NULL AND teamwork_base_url IS NULL) OR
							(teamwork_api_key IS NOT NULL AND teamwork_base_url IS NOT NULL)
						)
					);
				", &[])?;
				conn.execute("
					INSERT INTO metadata (version) VALUES (0);
				", &[])?;
				conn.execute("
					CREATE TABLE project (
						id TEXT PRIMARY KEY,
						name TEXT NOT NULL
					);
				", &[])?;
				conn.execute("
					CREATE TABLE timeblock (
						id TEXT PRIMARY KEY,
						project_id TEXT NOT NULL REFERENCES project(id),
						start TIMESTAMP NOT NULL,
						end TIMESTAMP
					);
				", &[])?;
			}
			_ => {
			}
		}
		
		try!(conn.execute("
			UPDATE metadata SET version=?
		", &[&v]));
	}

	Ok(vto)
}

fn dispatch(m: &clap::ArgMatches, s: &TimeTracker) {
	match m.subcommand_name() {
		Some("down") => {
			s.down();
		}
		Some("projects") => {
			s.projects();
		}
		_ => {
			panic!("No commands specified.");
		}
	}
}


fn main2() -> Result<(), Error> {
    let m = cli::build_cli().get_matches();
			
	//TODO Handle --db option here.
	let home_dir = std::env::home_dir().unwrap();
	let mut path = std::path::PathBuf::from(home_dir);
	path.push("/src/.tt.sqlite");
	let conn = Connection::open(path.as_path())?;
	//TODO Handle choosing the sub-system
	upgrade(&conn, 0);
	let t = teamwork::Teamwork::new(&conn, path.as_path())?;
	dispatch(&m, &t);
	Ok(())
}

fn main() {
	match main2() {
		Ok(_) => { }
		Err(e) => panic!("error: {:?}", e)
	}
}
