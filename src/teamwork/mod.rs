use std::io::{
	Write
};
use rusqlite::Connection;
use hyper::{
	Method, Request,
};
use rpassword::read_password;

use futures::future::{
	Future,
};
use futures::{
	Stream,
};

use super::{ TimeTracker, Project, Error };
use super::std;
use super::hyper;
use super::serde_json;
use super::serde_json::{ Value };
use super::time::{ Duration };
use super::chrono::{ 
	DateTime,
	Utc,
};

use super::hyper_tls;
use super::tokio_core;

use futures::future;
use futures::stream;

use super::ProjectDataSource;
use super::TimeblockDataSource;

pub struct Teamwork<'a> {
	conn: &'a Connection,
	api_key: String,
	base_url: String,
	user_id: i32
}

header! { (XPage, "X-Page") => [i16] }
header! { (XPages, "X-Pages") => [i16] }

impl<'a> TimeTracker for Teamwork<'a> {
	fn conn(&self) -> &Connection {
		self.conn
	}
	fn down(&self) -> Result<(), Error> {
		let psrc: &ProjectDataSource = self.conn;
		let tsrc: &TimeblockDataSource = self.conn;

		let last_sync = match tsrc.last_sync()? {
			Some(t) => {
				Some(t.format("%Y%m%d"))
			}
			_ => {
				None
			}
		};

		let mut core = tokio_core::reactor::Core::new().unwrap();
		let handle = core.handle();
		let client = hyper::Client::configure()
			.connector(hyper_tls::HttpsConnector::new(4, &handle).unwrap())
			.build(&handle);

		let mut page = 1;
		let mut num_pages = 1;

		while page <= num_pages {
			eprintln!("Teamwork.down: get project entries page {}/{}...", page, num_pages);
			let req = match last_sync {
				Some(ref t) => {
					self.get(format!("/projects.json?page={}&updatedAfterDate={}", page, t).to_string())?
				}
				_ => {
					self.get(format!("/projects.json?page={}", page).to_string())?
				}
			};

			let work = client.request(req).and_then(|res| {
				assert_eq!(res.status(), hyper::Ok);

				#[derive(Deserialize, Debug)]
				struct TeamworkProject {
					id: String,
					name: String
				}
				impl TeamworkProject {
					fn pid(&self) -> String {
						format!("/projects/{}", self.id)
					}
				}

				#[derive(Deserialize, Debug)]
				struct TeamworkProjectsResult {
					#[serde(rename="STATUS")]
					status: String,
					projects: Vec<TeamworkProject>
				}

				Teamwork::body(res).and_then(|s| {
					//println!("{:?}", s);
			 		let r = serde_json::from_str::<TeamworkProjectsResult>(&s).unwrap();
					let x = r.projects.into_iter().map(|t| {
						let n = t.name.clone();
						psrc.upsert(n, t.pid(), None)
					});
					stream::iter_ok::<_, hyper::Error>(x).collect()
				})
			});
			core.run(work).unwrap();
			page = page+1;
		}
		
		page = 1;
		num_pages = 1;

		while page <= num_pages {
			eprintln!("Teamwork.down: get task entries page {}/{}...", page, num_pages);
			let req = match last_sync {
				Some(ref t) => {
					self.get(format!("/tasks.json?page={}&showDeleted=yes&includeCompletedTasks=true&includeCompletedSubtasks=true&updatedAfterDate={}", page, t).to_string())?
				}
				_ => {
					self.get(format!("/tasks.json?page={}&showDeleted=yes&includeCompletedTasks=true&includeCompletedSubtasks=true", page).to_string())?
				}
			};

			let work = client.request(req).and_then(|res| {
				//assert_eq!(res.status(), hyper::Ok);
				Teamwork::body(res).and_then(|s|{
					//println!("{:?}", s);
					#[derive(Deserialize, Debug)]
					struct TeamworkTask {
						id: serde_json::Value,
						#[serde(rename="content")]
						name: String,
						#[serde(rename="project-id")]
						project: i32
					}
					impl TeamworkTask {
						fn pid(&self) -> String {
							format!("/tasks/{}", self.id)
						}
						fn ppid(&self) -> String {
							format!("/projects/{}", self.project)
						}
					}

					#[derive(Deserialize, Debug)]
					struct TeamworkTasksResult {
						#[serde(rename="STATUS")]
						status: String,
						#[serde(rename="todo-items")]
						tasks: Vec<TeamworkTask>
					}

					let r = serde_json::from_str::<TeamworkTasksResult>(&s).unwrap();
					let x = r.tasks.into_iter().map(|t| {
						let n = t.name.clone();
						let pref = super::ProjectRef::RemoteId(t.ppid());
						let p = psrc.get(pref, None)?;
						match p {
							Some(pp) => { 
								psrc.upsert(n, t.pid(), Some(pp.ev.eid))
							}
							_ => {
								Err(Error::TTError(format!(
									"Failed finding project by ID for: {:?}", t
								)))
							}
						}
					});
					stream::iter_ok::<_, hyper::Error>(x).collect()
				})
			});
			core.run(work).unwrap();
			page = page+1;
		}
		
		page = 1;
		num_pages = 1;

		while page <= num_pages {
			eprintln!("Teamwork.down: get time entries page {}/{}...", page, num_pages);

			let req = match last_sync {
				Some(ref t) => {
					self.get(format!("/time_entries.json?page={}&userId={}&updatedAfterDate={}", page, self.user_id, t).to_string())?
				}
				_ => {
					self.get(format!("/time_entries.json?page={}&userId={}", page, self.user_id).to_string())?
				}
			};

			let work = client.request(req).and_then(|mut res| {
				assert_eq!(res.status(), hyper::Ok);

				let h = res.headers_mut().clone();
				num_pages = match h.get::<XPages>().unwrap() {
					&XPages(i) => { i }
				};

				#[derive(Deserialize, Debug)]
				struct TeamworkTimeEntry {
					id: String,
					#[serde(rename="project-id")]
					project_id: String,
					#[serde(rename="todo-item-id")]
					task_id: String,
					minutes: serde_json::value::Value,
					isbillable: String,
					date: String,
					hours: serde_json::value::Value,
					#[serde(rename="person-last-name")]
					blah: String,
					#[serde(rename="todo-item-name")]
					blah2: String,
				}

				#[derive(Deserialize, Debug)]
				struct TeamworkTimeEntriesResult {
					#[serde(rename="STATUS")]
					status: String,
					#[serde(rename="time-entries")]
					entries: Vec<TeamworkTimeEntry>
				}

				Teamwork::body(res).and_then(|s| {
					let r = serde_json::from_str::<TeamworkTimeEntriesResult>(&s).unwrap();
					let x = r.entries.into_iter().map(|e| {
						//2016-01-01T06:17:00Z
						let start = e.date.parse::<DateTime<Utc>>().unwrap();
						//println!("{:?}", e);
						let end = match (e.hours, e.minutes) {
							(Value::String(hs), Value::String(ms)) => {
								let h = hs.parse::<i64>().unwrap();
								let m = ms.parse::<i64>().unwrap();
								Some(start + Duration::hours(h) + Duration::minutes(m))
							}
							(Value::Number(h), Value::Number(m)) => {
								Some(start + Duration::hours(h.as_i64().unwrap()) + Duration::minutes(m.as_i64().unwrap()))
							}
							_ => {
								None
							}
						};
						let mut billable = false;
						if e.isbillable == "True" {
							billable = true;
						}
						let pref = if e.task_id != "" {
							super::ProjectRef::RemoteId(format!("/tasks/{}", e.task_id))
						} else {
							super::ProjectRef::RemoteId(format!("/projects/{}", e.project_id))
						};
						let tb = tsrc.upsert(
							None,
							Some(e.id),
							pref,
							start,
							end,
							billable,
							"".to_string(),
							vec![],
							true
						);
					});
					stream::iter_ok::<_, hyper::Error>(x).collect()
				})
			});
			core.run(work).unwrap();
			page = page+1;
		}

		Ok(())
	}

	fn up(&self) -> Result<(), Error> {
		Ok(())
	}
}

impl<'a> Teamwork<'a> {
	pub fn new(conn: &'a Connection) -> Result<Teamwork<'a>, Error> {
		let mut stmt = conn.prepare("SELECT teamwork_api_key, teamwork_base_url, teamwork_user_id FROM metadata")?;
		
		let mut api_key = None;
		let mut base_url = None;
		let mut user_id: Option<i32> = None;

		stmt.query_map(&[], |row| {
			api_key = row.get(0);
			base_url = row.get(1);
			user_id = row.get(2);
		}).unwrap().next();

		match base_url {
			Some(_) => { }
			_ => {
				print!("Teamwork Base URL: ");
				std::io::stdout().flush()?;
				let s = read_password()?;
				base_url = Some(s);
			}
		}
		
		match user_id {
			Some(_) => { }
			_ => {
				print!("Teamwork User ID: ");
				std::io::stdout().flush()?;
				let s = read_password()?;
				user_id = Some(s.parse::<i32>().unwrap());
			}
		}

		match api_key {
			Some(_) => { }
			_ => {
				print!("Teamwork API Key: ");
				std::io::stdout().flush()?;
				let s = read_password()?;
				api_key = Some(s);
				
			}
		}
		
		let a = api_key.unwrap();
		let b = base_url.unwrap();
		let c = user_id.unwrap();
				
		conn.execute("UPDATE metadata SET teamwork_api_key=?, teamwork_base_url=?, teamwork_user_id=?", &[
			&a,
			&b,
			&c
		])?;

		Ok(Teamwork {
			conn: conn,
			api_key: a.clone(),
			base_url: b.clone(),
			user_id: c.clone(),
		})
	}

	pub fn get(&self, uri: String) -> Result<hyper::Request, hyper::Error> {
		println!("GET {}", uri);
		let hdr = hyper::header::Basic {
			username: self.api_key.clone(),
			password: Some("xxx".to_string())
		};

		let uri = format!("{}{}", self.base_url.clone(), uri).parse()?;
		let mut req = Request::new(Method::Get, uri);
		req.headers_mut().set(hyper::header::Accept::json());
		req.headers_mut().set(hyper::header::ContentType::json());
		req.headers_mut().set(hyper::header::Authorization(hdr.clone()));
		Ok(req)
	}

	pub fn body(res: hyper::Response) -> Box<future::Future<Item=String, Error=hyper::Error> + Send> {
		Box::new(res.body().fold(Vec::new(), |mut v, chunk| {
			v.extend(&chunk[..]);
			future::ok::<_, hyper::Error>(v)
		}).and_then(|chunks| {
			let s = String::from_utf8(chunks).unwrap().clone();
			future::ok::<_, hyper::Error>(s)
		}))
	}
}
