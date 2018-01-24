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

use super::{ System, Project };
use super::std;
use super::rusqlite;
use super::hyper;
use super::serde_json;

use super::hyper_tls;
use super::tokio_core;

use futures::future;
use futures::stream;

pub struct Teamwork<'a> {
	path: &'a std::path::Path,
	conn: &'a Connection,
	api_key: Option<String>,
	base_url: Option<String>
}

impl<'a> Teamwork<'a> {
	pub fn new(conn: &'a Connection, path: &'a std::path::Path, vto: i32) -> Result<Teamwork<'a>, rusqlite::Error> {
		let res = System::upgrade(conn, vto, &|conn, v| {
			match v {
				0 => {
					try!(conn.execute("
						ALTER TABLE metadata ADD COLUMN api_key TEXT NULL;
					", &[]));
					Ok(())
				}
				1 => {
					try!(conn.execute("
						ALTER TABLE metadata ADD COLUMN base_url TEXT NULL;
					", &[]));
					Ok(())
				}
				_ => {
					return Err(rusqlite::Error::QueryReturnedNoRows);
				}
			}
		});
		match res {
			Ok(_) => {
				let mut stmt = conn.prepare("SELECT api_key, base_url FROM metadata WHERE apiKey IS NOT NULL")?;
				let mut twi = stmt.query_map(&[], |row| {
					Teamwork {
						conn: conn,
						path: path,
						api_key: row.get(0),
						base_url: row.get(1)
					}
				}).unwrap();
				twi.next().unwrap()

			}
			Err(e) => Err(e)
		}
	}

	pub fn get(&self, uri: String) -> Result<hyper::Request, hyper::Error> {
		let hdr = hyper::header::Basic {
			username: self.api_key.clone().unwrap(),
			password: Some("xxx".to_string())
		};

		let uri = format!("{}/{}", self.base_url.clone().unwrap(), uri).parse()?;
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

impl<'a> System<'a> for Teamwork<'a> {
	fn setup(&self) -> Result<(), super::Error> {
		print!("API Key: ");
		std::io::stdout().flush()?;
		let input = read_password()?;
		self.conn.execute("UPDATE metadata SET api_key=?", &[&input])?;

		print!("Teamwork Base URL: ");
		std::io::stdout().flush()?;
		let input = read_password()?;
		self.conn.execute("UPDATE metadata SET base_url=?", &[&input])?;

		Ok(())
	}

	fn projects(&self) -> Result<(), hyper::error::Error> {
		match self.api_key.clone() {
			Some(_) => {
				//let mut core = Core::new()?;
				//let client = Client::new(&core.handle());
				
				let mut core = tokio_core::reactor::Core::new().unwrap();
				let handle = core.handle();
				let client = hyper::Client::configure()
					.connector(hyper_tls::HttpsConnector::new(4, &handle).unwrap())
					.build(&handle);

				let mut req = self.get("/projects.json".to_string())?;

				let work = client.request(req).and_then(|res| {
					assert_eq!(res.status(), hyper::Ok);

					#[derive(Deserialize, Debug)]
					struct TeamworkProjectsResult {
						#[serde(rename="STATUS")]
						status: String,
						projects: Vec<Project>
					}

					Teamwork::body(res).and_then(|s| {
						let r = serde_json::from_str::<TeamworkProjectsResult>(&s).unwrap();
						let x = r.projects.into_iter();
						stream::iter_ok::<_, hyper::Error>(x).collect()
					})
				});
				let twprojects = core.run(work).unwrap();

				twprojects.into_iter().map(|p|{
					println!("{:?} > Tasks: ", p.name);
					let req = self.get(format!("/projects/{}/tasks.json", p.id).to_string()).unwrap();
					let work = client.request(req).and_then(|res| {
						assert_eq!(res.status(), hyper::Ok);
						Teamwork::body(res).and_then(|s|{

							#[derive(Deserialize, Debug)]
							struct TeamworkTask {
								id: u32,
								#[serde(rename="content")]
								name: String
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
								Project {
									id: format!("{}", t.id),
									name: format!("{} > {}", p.name, t.name)
								}
							});
							stream::iter_ok::<_, hyper::Error>(x).collect()
						})
					});
					let twtasks = core.run(work).unwrap();
					for t in twtasks {
						println!("{:?}", t);
					}
				}).collect::<()>();
			}
			None => {
				panic!("No API key specified.");
			}
		}
		Ok(())
	}
}


