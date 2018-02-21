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
#[derive(Debug)]
pub enum ProjectRef {
	EV(EntityVersion),
	EId(DbId),
	RemoteId(RemoteId),
	Obj(Project)
}
static SQL_0_0: [&'static str; 2] = ["
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

		UNIQUE (eid, vid)
	);
"
];
pub trait ProjectDataSource {
	fn upsert(&self, name: String, remote_id: RemoteId, parent_eid: Option<DbId>) -> Result<Project, Error>;
	fn get(&self, proj: ProjectRef, when: Option<Timespec>) -> Result<Option<Project>, rusqlite::Error>;
	fn list(&self, when: Option<Timespec>) -> Result<Vec<Project>, Error>;
	fn parents(&self, proj: ProjectRef, when: Option<Timespec>) -> Result<Vec<Project>, Error>;
}

impl ProjectDataSource for rusqlite::Connection {
	fn upsert(&self, name: String, remote_id: RemoteId, parent_eid: Option<DbId>) -> Result<Project, Error> {
		let psrc: &ProjectDataSource = self;
		match psrc.get(ProjectRef::RemoteId(remote_id.clone()), None)? {
			Some(p) => {
				let vtime = time::get_time();
				let vid = p.ev.vid+1;
				self.prepare("INSERT INTO project (remote_id, name, parent_eid, eid, vid, vtime) VALUES (?, ?, ?, ?, ?, ?)")?.execute(&[&remote_id, &name, &parent_eid.to_sql()?, &p.ev.eid, &vid, &vtime])?;
				Ok(Project {
					remote_id: remote_id,
					name: name,
					parent_eid: parent_eid,
					alive: true,
					ev: EntityVersion {
						eid: p.ev.eid,
						vid: vid,
						vtime: vtime
					}
				})
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

				let t = format!("{:?}", when.unwrap_or(time::get_time()));
				let x = stmt.query_map(&[&a, &t], |row| {
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
				})?.next().unwrap_or(Ok(None));
				x
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
	fn parents(&self, proj: ProjectRef, when: Option<Timespec>) -> Result<Vec<Project>, Error> {
		let psrc: &ProjectDataSource = self;
		let mut cur = psrc.get(proj, when)?;
		let mut output = Vec::new();
		while cur.is_some() {
			let p = cur.unwrap();
			match p.parent_eid {
				Some(eid) => {
					cur = psrc.get(ProjectRef::EId(eid), when)?
				}
				_ => {
					cur = None
				}
			}
			output.push(p);
		}
		output.reverse();
		Ok(output)
	}
}

#[derive(Debug)]
pub struct Timeblock {
	remote_id: Option<RemoteId>,
	project: ProjectRef,
	start: Timespec,
	end: Option<Timespec>,
	billable: bool,
	notes: String,
	tags: Vec<String>,
	alive: bool,
	ev: EntityVersion
}
#[derive(Debug)]
pub enum TimeblockRef {
	EV(EntityVersion),
	EId(DbId),
	RemoteId(RemoteId),
	Obj(Timeblock)
}
static SQL_0_1: [&'static str; 2] = ["
	CREATE TABLE timeblock_entity (
		id INTEGER PRIMARY KEY,
		last_sync_vid INTEGER DEFAULT NULL,
		last_sync_time TIMESTAMP DEFAULT NULL
	);
","
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
"];

pub enum TimeblockFilter<'a> {
	Ref(TimeblockRef),
	And(&'a TimeblockFilter<'a>, &'a TimeblockFilter<'a>),
	Or(&'a TimeblockFilter<'a>, &'a TimeblockFilter<'a>),
	Project(Option<ProjectRef>),
	Open(bool),
	Tag(String),
	AtTime(Timespec),
}
impl<'a> TimeblockFilter<'a> {
	fn where_clause(&'a self) -> (String, Vec<&rusqlite::types::ToSql>) {
		match *self {
			TimeblockFilter::Ref(ref tb) => {
				match tb {
					&TimeblockRef::EV(ref ev) => {
						("(tb.eid=?)".to_string(), vec![&ev.eid])
					}
					&TimeblockRef::EId(ref eid) => {
						("(tb.eid=?)".to_string(), vec![eid])
					}
					&TimeblockRef::RemoteId(ref remote_id) => {
						("(tb.remote_id=?)".to_string(), vec![remote_id])
					}
					&TimeblockRef::Obj(ref tb) => {
						("(tb.eid=?)".to_string(), vec![&tb.ev.eid])
					}
				}
			}
			TimeblockFilter::Project(ref p) => {
				match p {
					&Some(ProjectRef::EV(ref ev)) => {
						("(p.eid=?)".to_string(), vec![&ev.eid])
					}
					&Some(ProjectRef::EId(ref eid)) => {
						("(p.eid=?)".to_string(), vec![eid])
					}
					&Some(ProjectRef::RemoteId(ref remote_id)) => {
						("(p.eid=?)".to_string(), vec![remote_id])
					}
					&Some(ProjectRef::Obj(ref p)) => {
						("(p.eid=?)".to_string(), vec![&p.ev.eid])
					}
					_ => {
						("1".to_string(), Vec::new())
					}
				}
			}
			TimeblockFilter::And(a, b) => {
				let (at, mut aa) = a.where_clause();
				let (bt, ba) = b.where_clause();
				aa.extend(ba);
				(format!("({} AND {})", at, bt), aa)
			}
			TimeblockFilter::Or(a, b) => {
				let (at, mut aa) = a.where_clause();
				let (bt, ba) = b.where_clause();
				aa.extend(ba);
				(format!("({} OR {})", at, bt), aa)
			}
			TimeblockFilter::Open(true) => {
				("(tb.end IS NULL)".to_string(), Vec::new())
			}
			TimeblockFilter::Open(false) => {
				("(tb.end IS NOT NULL)".to_string(), Vec::new())
			}
			TimeblockFilter::Tag(ref tag) => {
				("(tb.tags LIKE ?)".to_string(), Vec::new())
			}
			TimeblockFilter::AtTime(ref t) => {
				("(tb.vtime <= ?)".to_string(), vec![t])
			}
		}
	}
}

pub trait TimeblockDataSource {
	fn upsert(&self, tb: Option<TimeblockRef>, remote_id: Option<RemoteId>, project: ProjectRef, start: Timespec, end: Option<Timespec>, billable: bool, notes: String, tags: Vec<String>, alive: bool) -> Result<Timeblock, Error>;
	fn get(&self, tb: TimeblockRef, when: Option<Timespec>) -> Result<Option<Timeblock>, rusqlite::Error>;
	fn search(&self, filter: Option<TimeblockFilter>) -> Result<Vec<Timeblock>, rusqlite::Error>;
}

impl TimeblockDataSource for rusqlite::Connection {
	fn search(&self, filter: Option<TimeblockFilter>) -> Result<Vec<Timeblock>, rusqlite::Error> {
		let q = filter.unwrap_or(TimeblockFilter::AtTime(time::get_time()));
		let (where_clause, args) = q.where_clause();
		let sql = format!("SELECT tb.* FROM timeblock AS tb WHERE tb.vid IN (SELECT MAX(vid) FROM timeblock AS tb2 WHERE tb2.eid=tb.eid) INNER JOIN project AS p ON p.eid=tb.project_eid AND {}", where_clause);

		let a = args.as_slice();
		let mut stmt = self.prepare(sql.as_str())?;
		let out = stmt.query_map(a, |row| {
			Timeblock {
				remote_id: row.get(0),
				project: ProjectRef::EId(row.get(1)),
				start: row.get(2),
				end: row.get(3),
				billable: row.get(4),
				notes: row.get(5),
				tags: vec![row.get(6)],
				alive: row.get(7),
				ev: EntityVersion {
					eid: row.get(8),
					vid: row.get(9),
					vtime: row.get(10)
				}
			}
		})?.map(|x| x.unwrap()).collect();
		Ok(out)
	}

	fn upsert(&self, tb: Option<TimeblockRef>, remote_id: Option<RemoteId>, project: ProjectRef, start: Timespec, end: Option<Timespec>, billable: bool, notes: String, tags: Vec<String>, alive: bool) -> Result<Timeblock, Error> {
		let psrc: &ProjectDataSource = self;
		let tbsrc: &TimeblockDataSource = self;
		let proj = psrc.get(project, None)?.unwrap();
		let g = match tb {
			Some(tb) => { tbsrc.get(tb, None)? }
			None => None
		};
		match g {
			Some(tb) => {
				let vtime = time::get_time();
				let vid = tb.ev.vid+1;
				let t = tags.join("\n").to_string();
				self.prepare("INSERT INTO timeblock (remote_id, project_eid, start, end, billable, notes, tags, alive, eid, vid, vtime) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?")?.execute(&[&remote_id, &proj.ev.eid, &start, &end, &billable, &notes, &t, &alive, &tb.ev.eid, &vid, &vtime])?;
				Ok(Timeblock {
					remote_id: remote_id,
					project: ProjectRef::EId(proj.ev.eid),
					start: start,
					end: end,
					billable: billable,
					notes: notes,
					tags: tags,
					alive: alive,
					ev: EntityVersion {
						eid: tb.ev.eid,
						vid: vid,
						vtime: vtime
					}
				})
			}
			_ => {
				self.prepare("INSERT INTO timeblock_entity VALUES (NULL, NULL, NULL)")?.execute(&[])?;
				let eid: DbId = self.last_insert_rowid();
				let vtime = time::get_time();
				let vid = 0;
				let t = tags.join("\n").to_string();
				self.prepare("INSERT INTO timeblock (remote_id, project_eid, start, end, billable, notes, tags, alive, eid, vid, vtime) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")?.execute(&[&remote_id, &proj.ev.eid, &start, &end, &billable, &notes, &t, &alive, &eid, &vid, &vtime])?;
				Ok(Timeblock {
					remote_id: remote_id,
					project: ProjectRef::EId(proj.ev.eid),
					start: start,
					end: end,
					billable: billable,
					notes: notes,
					tags: tags,
					alive: alive,
					ev: EntityVersion {
						eid: eid,
						vid: vid,
						vtime: vtime
					}
				})
			}
		}
	}

	fn get(&self, tb: TimeblockRef, when: Option<Timespec>) -> Result<Option<Timeblock>, rusqlite::Error> {
		match tb {
			TimeblockRef::Obj(tb) => {
				Ok(Some(tb))
			}
			_ => {
				let mut stmt = match tb {
					TimeblockRef::EV(_) => {
						self.prepare("SELECT p.* FROM timeblock AS p WHERE p.eid=? AND p.vid IN (SELECT MAX(vid) FROM timeblock AS p2 WHERE p2.eid=p.eid AND p2.vtime <= ?) LIMIT 1")?
					}
					TimeblockRef::EId(_) => {
						self.prepare("SELECT p.* FROM timeblock AS p WHERE p.eid=? AND p.vid IN (SELECT MAX(vid) FROM timeblock AS p2 WHERE p2.eid=p.eid AND p2.vtime <= ?) LIMIT 1")?
					}
					TimeblockRef::RemoteId(_) => {
						self.prepare("SELECT p.* FROM timeblock AS p WHERE p.remote_id=? AND p.vid IN (SELECT MAX(vid) FROM timeblock AS p2 WHERE p2.eid=p.eid AND p2.vtime <= ?) LIMIT 1")?
					}
					TimeblockRef::Obj(_) => {
						panic!("Impossible")
					}
				};
				let a = match tb {
					TimeblockRef::EV(ev) => { format!("{}", ev.eid) }
					TimeblockRef::EId(eid) => { format!("{}", eid) }
					TimeblockRef::RemoteId(remote_id) => { remote_id.clone() }
					TimeblockRef::Obj(_) => { panic!("Impossible") }
				};

				let t = format!("{:?}", when.unwrap_or(time::get_time()));
				let x = stmt.query_map(&[&a, &t], |row| {
					Some(Timeblock {
						remote_id: row.get(0),
						project: ProjectRef::EId(row.get(1)),
						start: row.get(2),
						end: row.get(3),
						billable: row.get(4),
						notes: row.get(5),
						tags: vec![row.get(6)],
						alive: row.get(7),
						ev: EntityVersion {
							eid: row.get(8),
							vid: row.get(9),
							vtime: row.get(10)
						}
					})
				})?.next().unwrap_or(Ok(None));
				x
			}
		}
	}
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
	fn conn(&self) -> &Connection;

	fn down(&self) -> Result<(), Error>;
	fn up(&self) -> Result<(), Error>;

	fn punchin(&self, proj: &Project) -> Result<(), Error> {
		let t: &TimeblockDataSource = self.conn();
		t.upsert(None, None, ProjectRef::EId(proj.ev.eid), time::get_time(), None, false, "".to_string(), vec![], true)?;
		Ok(())
	}
	
	fn punchout(&self, proj: Option<&Project>) -> Result<(), Error> {
		let t: &TimeblockDataSource = self.conn();
		let p: &ProjectDataSource = self.conn();

//		t.search(TimeblockFilter::Open(true)).filter(|tb| tb.project == 

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
				conn.execute(SQL_0_0[0], &[])?;
				conn.execute(SQL_0_0[1], &[])?;
				conn.execute(SQL_0_1[0], &[])?;
				conn.execute(SQL_0_1[1], &[])?;
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
		("punchout", Some(punchout_matches)) => {
			let name = punchout_matches.value_of("project");
			let projects = s.conn().list(None)?;
			let proj = match name {
				Some(name) => {
					let index = projects.iter().position(|p| {
						p.name == name || p.remote_id == name
					}).unwrap();
					projects.get(index)
				}
				_ => None
			};
			let t = s.punchout(proj)?;
			println!("{:?}", t);
		}
		("projects", Some(_)) => {
			for p in s.conn().list(None)? {
				let remote_id = p.remote_id.clone();
				let parents = s.conn().parents(ProjectRef::Obj(p), None)?;
				let names: Vec<String> = parents.iter().map(|x| x.name.clone()).collect();
				println!("{}: {}", remote_id, names.join(" > "));
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
	let search = vec![std::env::home_dir().unwrap(), std::env::current_dir().unwrap()];
	let mut conn: Option<Connection> = None;
	for dir in search {
		let mut path = std::path::PathBuf::from(dir);
		path.push(".tt.sqlite");
		conn = match Connection::open(path.as_path()) {
			Ok(c) => { Some(c) }
			_ => { None }
		};
		if conn.is_some() {
			break
		}
	}
	//TODO Handle choosing the sub-system
	let c = &conn.unwrap();
	upgrade(c, 0)?;
	let t = teamwork::Teamwork::new(c)?;
	Ok(dispatch(&m, &t)?)
}

fn main() {
	match main2() {
		Ok(_) => { }
		Err(e) => panic!("error: {:?}", e)
	}
}
