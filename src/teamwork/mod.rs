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
use super::time;
use super::time::{ Duration };

use super::hyper_tls;
use super::tokio_core;

use futures::future;
use futures::stream;

use super::ProjectDataSource;
use super::TimeblockDataSource;

pub struct Teamwork<'a> {
	conn: &'a Connection,
	api_key: String,
	base_url: String
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

		let mut core = tokio_core::reactor::Core::new().unwrap();
		let handle = core.handle();
		let client = hyper::Client::configure()
			.connector(hyper_tls::HttpsConnector::new(4, &handle).unwrap())
			.build(&handle);

		let mut page = 1;
		let mut num_pages = 1;

		while page <= num_pages {
			eprintln!("Teamwork.down: get project entries page {}/{}...", page, num_pages);

			let req = self.get(format!("/projects.json?page={}", page).to_string())?;

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
					let r = serde_json::from_str::<TeamworkProjectsResult>(&s).unwrap();
					let x = r.projects.into_iter();
					stream::iter_ok::<_, hyper::Error>(x).collect()
				})
			});
			let twprojects = core.run(work).unwrap();

			twprojects.into_iter().map(|p|{
				let n = p.name.clone();
				let pp = match psrc.upsert(n, p.pid(), None) {
					Ok(p) => { p }
					Err(e) => {
						panic!("{:?}", e);
					}
				};

				let req = self.get(format!("/projects/{}/tasks.json", p.id).to_string()).unwrap();
				let work = client.request(req).and_then(|res| {
					assert_eq!(res.status(), hyper::Ok);
					Teamwork::body(res).and_then(|s|{
						#[derive(Deserialize, Debug)]
						struct TeamworkTask {
							id: serde_json::Value,
							#[serde(rename="content")]
							name: String
						}
						impl TeamworkTask {
							fn pid(&self) -> String {
								format!("/tasks/{}", self.id)
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
							psrc.upsert(n, t.pid(), Some(pp.ev.eid))
						});
						stream::iter_ok::<_, hyper::Error>(x).collect()
					})
				});
				core.run(work).unwrap();
			}).collect::<()>();
			page = page+1;
		}
		
		page = 1;
		num_pages = 1;

		while page <= num_pages {
			eprintln!("Teamwork.down: get time entries page {}/{}...", page, num_pages);

			let req = self.get(format!("/time_entries.json?page={}", page).to_string())?;

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
						let start = super::time::strptime(&e.date, "%Y-%m-%dT%H:%M:%S%z").unwrap().to_timespec();
						let end = match (e.hours, e.minutes) {
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
						let pref = if (e.task_id != "") {
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
		let mut stmt = conn.prepare("SELECT teamwork_api_key, teamwork_base_url FROM metadata")?;
		
		let mut api_key = None;
		let mut base_url = None;

		stmt.query_map(&[], |row| {
			api_key = row.get(0);
			base_url = row.get(1);
		}).unwrap().next();

		match api_key {
			Some(_) => { }
			_ => {
				print!("Teamwork API Key: ");
				std::io::stdout().flush()?;
				let s = read_password()?;
				api_key = Some(s);
				
			}
		}

		match base_url {
			Some(_) => { }
			_ => {
				print!("Teamwork Base URL: ");
				std::io::stdout().flush()?;
				let s = read_password()?;
				base_url = Some(s);
			}
		}
		
		let a = api_key.unwrap();
		let b = base_url.unwrap();
				
		conn.execute("UPDATE metadata SET teamwork_api_key=?, teamwork_base_url=?", &[
			&a,
			&b
		])?;

		Ok(Teamwork {
			conn: conn,
			api_key: a.clone(),
			base_url: b.clone()
		})
	}

	pub fn get(&self, uri: String) -> Result<hyper::Request, hyper::Error> {
		let hdr = hyper::header::Basic {
			username: self.api_key.clone(),
			password: Some("xxx".to_string())
		};

		let uri = format!("{}/{}", self.base_url.clone(), uri).parse()?;
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
