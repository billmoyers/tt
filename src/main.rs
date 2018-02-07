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
use rusqlite::types::ToSql;

type RemoteId = String;
type DbId = i64;

#[derive(Debug)]
pub struct EntityVersion {
	eid: DbId,
	vid: DbId,
	vtime: Timespec
}

#[derive(Debug)]
pub struct Project {
	remote_id: RemoteId,
	name: String,
	parent_eid: Option<DbId>,
	alive: bool,
	ev: EntityVersion
}
pub enum ProjectRef {
	EV(EntityVersion),
	EId(DbId),
	RemoteId(RemoteId),
	Obj(Project)
}
static SQL_0: [&'static str; 2] = ["
	CREATE TABLE project_entity (
		id INTEGER PRIMARY KEY
	);
","
	CREATE TABLE project (
		remote_id TEXT NOT NULL,
		name TEXT NOT NULL,
		parent_eid INTEGER DEFAULT NULL REFERENCES project_entity(id),
		alive BOOLEAN DEFAULT 1,

		eid INTEGER NOT NULL REFERENCES project_entity(id),
		vid INTEGER NOT NULL,
		vtime TIMESTAMP NOT NULL,

		UNIQUE (eid, vid),
		UNIQUE (eid, remote_id)
	);
"
];
pub trait ProjectDataSource {
	fn upsert(&self, name: String, remote_id: RemoteId, parent_eid: Option<DbId>) -> Result<Project, Error>;
	fn get(&self, proj: ProjectRef, when: Option<Timespec>) -> Result<Option<Project>, rusqlite::Error>;
	fn list(&self, when: Option<Timespec>) -> Result<Vec<Project>, Error>;
}

impl ProjectDataSource for rusqlite::Connection {
	fn upsert(&self, name: String, remote_id: RemoteId, parent_eid: Option<DbId>) -> Result<Project, Error> {
		match self.get(ProjectRef::RemoteId(remote_id.clone()), None)? {
			Some(_) => {
				//Project.update(conn, p)
				Err(Error::TTError("not implemented".to_string()))
			}
			_ => {
				self.prepare("INSERT INTO project_entity VALUES (NULL)")?.execute(&[])?;
				let eid: DbId = self.last_insert_rowid();
				let vtime = time::get_time();
				let vid = 0;
				self.prepare("INSERT INTO project (remote_id, name, parent_eid, eid, vid, vtime) VALUES (?, ?, ?, ?, ?, ?)")?.execute(&[&remote_id, &name, &parent_eid.to_sql()?, &eid, &vid, &vtime])?;
				Ok(Project {
					remote_id: remote_id,
					name: name,
					parent_eid: parent_eid,
					alive: true,
					ev: EntityVersion {
						eid: eid,
						vid: vid,
						vtime: vtime
					}
				})
			}
		}
	}

	fn get(&self, proj: ProjectRef, when: Option<Timespec>) -> Result<Option<Project>, rusqlite::Error> {
		match proj {
			ProjectRef::Obj(p) => {
				Ok(Some(p))
			}
			_ => {
				let mut stmt = match proj {
					ProjectRef::EV(_) => {
						self.prepare("SELECT p.* FROM project AS p WHERE p.eid=? AND p.vid IN (SELECT MAX(vid) FROM project AS p2 WHERE p2.eid=p.eid AND p2.vtime <= ?) LIMIT 1")?
					}
					ProjectRef::EId(_) => {
						self.prepare("SELECT p.* FROM project AS p WHERE p.eid=? AND p.vid IN (SELECT MAX(vid) FROM project AS p2 WHERE p2.eid=p.eid AND p2.vtime <= ?) LIMIT 1")?
					}
					ProjectRef::RemoteId(_) => {
						self.prepare("SELECT p.* FROM project AS p WHERE p.remote_id=? AND p.vid IN (SELECT MAX(vid) FROM project AS p2 WHERE p2.eid=p.eid AND p2.vtime <= ?) LIMIT 1")?
					}
					ProjectRef::Obj(_) => {
						panic!("Impossible")
					}
				};
				let a = match proj {
					ProjectRef::EV(ev) => { format!("{}", ev.eid) }
					ProjectRef::EId(eid) => { format!("{}", eid) }
					ProjectRef::RemoteId(remote_id) => { remote_id.clone() }
					ProjectRef::Obj(_) => { panic!("Impossible") }
				};

				stmt.query_row(&[&a], |row| {
					Some(Project {
						remote_id: row.get(0),
						name: row.get(1),
						parent_eid: row.get(2),
						alive: row.get(3),
						ev: EntityVersion {
							eid: row.get(4),
							vid: row.get(5),
							vtime: row.get(6)
						}
					})
				})
			}
		}
	}
	fn list(&self, when: Option<Timespec>) -> Result<Vec<Project>, Error> {
		let mut stmt = self.prepare("SELECT p.* FROM project AS p WHERE p.vid IN (SELECT MAX(vid) FROM project AS p2 WHERE p2.eid=p.eid AND p2.vtime <= ?) ORDER BY eid")?;
		let t = match when {
			Some(t) => { t }
			_ => { time::get_time() }
		};

		let out = stmt.query_map(&[&t], |row| {
			Project {
				remote_id: row.get(0),
				name: row.get(1),
				parent_eid: row.get(2),
				alive: row.get(3),
				ev: EntityVersion {
					eid: row.get(4),
					vid: row.get(5),
					vtime: row.get(6)
				}
			}
		})?.map(|x| x.unwrap()).collect();
		Ok(out)
	}
}

#[derive(Debug)]
struct Timeblock {
	remote_id: Option<RemoteId>,
	project_eid: Option<DbId>,
	start: Timespec,
	end: Option<Timespec>,
	alive: bool,
	ev: EntityVersion
}

#[derive(Debug)]
pub enum Error {
	RusqliteError(rusqlite::Error),
	IOError(std::io::Error),
	HyperError(hyper::Error),
	TTError(String)
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
	//fn punchin(&self, proj: &Project) -> Result<(), Error>;
	//fn punchout(&self, tb: Timeblock) -> Result<Timeblock, Error>;

	fn conn(&self) -> &Connection;

	fn down(&self) -> Result<(), Error>;
	fn up(&self) -> Result<(), Error>;

//	fn get(conn: &Connection, eid: DbId) -> Result<Option<Project>, Error>;
//	fn update(conn: &Connection, proj: Project) -> Result<Project, Error>;
//	fn delete(conn: &Connection, proj: Project) -> Result<Project, Error>;

	fn punchin(&self, proj: &Project) -> Result<(), Error> {
		/*Ok(Timeblock {
			remote_id: None,
			project_id: proj.id.clone(),
			start: time::get_time(),
			end: None
		})*/
		Ok(())
	}
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
				conn.execute(SQL_0[0], &[])?;
				conn.execute(SQL_0[1], &[])?;
				conn.execute("
					CREATE TABLE timeblock_entity (
						id INTEGER PRIMARY KEY,
						last_sync_vid INTEGER,
						last_sync_time TIMESTAMP
					);
				", &[])?;
				conn.execute("
					CREATE TABLE timeblock (
						remote_id TEXT DEFAULT NULL,
						project_eid TEXT NOT NULL REFERENCES project_entity(id),
						start TIMESTAMP NOT NULL,
						end TIMESTAMP DEFAULT NULL,
						billable BOOLEAN,
						notes TEXT NOT NULL DEFAULT '',
						tags TEXT NOT NULL DEFAULT '',
						alive BOOLEAN DEFAULT 1,
						
						eid INTEGER NOT NULL REFERENCES timeblock_entity(id),
						vid INTEGER NOT NULL,
						vtime TIMESTAMP NOT NULL,

						CHECK (remote_id IS NULL OR end IS NOT NULL)
						UNIQUE (eid, vid),
						UNIQUE (eid, remote_id)
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

fn dispatch(m: &clap::ArgMatches, s: &TimeTracker) -> Result<(), Error> {
	match m.subcommand() {
		("down", Some(_)) => {
			s.down()?;
		}
		("punchin", Some(punchin_matches)) => {
			let name = punchin_matches.value_of("project").unwrap();
			let projects = s.conn().list(None)?;
			let index = projects.iter().position(|p| {
				p.name == name || p.remote_id == name
			}).unwrap();
			let mut proj = projects.get(index).unwrap();
			let t = s.punchin(proj)?;
			println!("{:?}", t);
		}
		("projects", Some(_)) => {
			for p in s.conn().list(None)? {
				println!("{}", p.name);
			}
		}
		_ => {
			Error::TTError("No commands specified.".to_string());
		}
	}
	Ok(())
}


fn main2() -> Result<(), Error> {
    let m = cli::build_cli().get_matches();
			
	//TODO Handle --db option here.
	let home_dir = std::env::home_dir().unwrap();
	let mut path = std::path::PathBuf::from(home_dir);
	path.push("/src/.tt.sqlite");
	let conn = Connection::open(path.as_path())?;
	//TODO Handle choosing the sub-system
	upgrade(&conn, 0)?;
	let t = teamwork::Teamwork::new(&conn)?;
	Ok(dispatch(&m, &t)?)
}

fn main() {
	match main2() {
		Ok(_) => { }
		Err(e) => panic!("error: {:?}", e)
	}
}
