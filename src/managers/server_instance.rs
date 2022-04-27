use crate::event_processor::{self, EventProcessor, PlayerEventVarient};
use crate::managers::properties_manager;
use rocket::serde::json::serde_json;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use systemstat::Duration;
// use std::sync::mpsc::{self, Receiver, Sender};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fmt, thread, time};
// use self::macro_code::dispatch_macro;

use super::macro_manager::MacroManager;
use super::properties_manager::PropertiesManager;

#[derive(Debug, Clone, Copy)]
pub enum Flavour {
    Vanilla,
    Fabric,
    Paper,
    Spigot,
}

impl<'de> Deserialize<'de> for Flavour {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "vanilla" => Ok(Flavour::Vanilla),
            "fabric" => Ok(Flavour::Fabric),
            "paper" => Ok(Flavour::Paper),
            "spigot" => Ok(Flavour::Spigot),
            _ => Err(serde::de::Error::custom(format!("Unknown flavour: {}", s))),
        }
    }
}
impl Serialize for Flavour {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Flavour::Vanilla => serializer.serialize_str("vanilla"),
            Flavour::Fabric => serializer.serialize_str("fabric"),
            Flavour::Paper => serializer.serialize_str("paper"),
            Flavour::Spigot => serializer.serialize_str("spigot"),
        }
    }
}

impl ToString for Flavour {
    fn to_string(&self) -> String {
        match self {
            Flavour::Vanilla => "vanilla".to_string(),
            Flavour::Fabric => "fabric".to_string(),
            Flavour::Paper => "paper".to_string(),
            Flavour::Spigot => "spigot".to_string(),
        }
    }
}

// impl fmt::Display for Flavour {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         match self {
//             Flavour::Vanilla => write!(f, "vanilla"),
//             Flavour::Fabric => write!(f, "fabric"),
//             Flavour::Paper => write!(f, "paper"),
//             Flavour::Spigot => write!(f, "spigot"),
//         }
//     }
// }

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Status {
    Starting,
    Stopping,
    Running,
    Stopped,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Status::Starting => write!(f, "Starting"),
            Status::Stopping => write!(f, "Stopping"),
            Status::Running => write!(f, "Running"),
            Status::Stopped => write!(f, "Stopped"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "rocket::serde")]

// this struct is neccessary to set up a server instance
pub struct InstanceConfig {
    pub name: String,
    pub version: String,
    pub flavour: Flavour,
    /// url to download the server.jar file from upon setup
    pub url: Option<String>,
    pub port: Option<u32>,
    pub uuid: Option<String>,
    pub min_ram: Option<u32>,
    pub max_ram: Option<u32>,
    pub creation_time: Option<u64>,
    pub auto_start: Option<bool>,
    pub restart_on_crash: Option<bool>,
    pub timeout_last_left: Option<i32>,
    pub timeout_no_activity: Option<i32>,
    pub start_on_connection: Option<bool>,
}

impl InstanceConfig {
    fn fill_default(&self) -> InstanceConfig {
        let mut config_override = self.clone();
        if self.auto_start == None {
            config_override.auto_start = Some(false);
        }
        if self.restart_on_crash == None {
            config_override.restart_on_crash = Some(false);
        }
        if self.creation_time == None {
            config_override.creation_time = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
        }
        if self.timeout_last_left == None {
            config_override.timeout_last_left = Some(-1);
        }
        if self.timeout_no_activity == None {
            config_override.timeout_no_activity = Some(-1);
        }
        if self.start_on_connection == None {
            config_override.start_on_connection = Some(false);
        }
        if self.max_ram == None {
            config_override.max_ram = Some(3000);
        }
        if self.min_ram == None {
            config_override.min_ram = Some(1000);
        }
        config_override
    }
}

pub struct ServerInstance {
    name: String,
    version: String,
    flavour: Flavour,
    port: u32,
    uuid: String,
    min_ram: u32,
    max_ram: u32,
    creation_time: u64,
    auto_start: Arc<Mutex<bool>>,
    restart_on_crash: Arc<Mutex<bool>>,
    timeout_last_left: Arc<Mutex<i32>>,
    timeout_no_activity: Arc<Mutex<i32>>,
    start_on_connection: Arc<Mutex<bool>>,
    jvm_args: Vec<String>,
    path: PathBuf,
    pub stdin: Arc<Mutex<Option<ChildStdin>>>,
    status: Arc<Mutex<Status>>,
    process: Option<Arc<Mutex<Child>>>,
    player_online: Arc<Mutex<Vec<String>>>,
    pub event_processor: Arc<Mutex<EventProcessor>>,
    properties_manager: PropertiesManager,
    macro_manager: Arc<Mutex<MacroManager>>,
    proxy_kill_tx: Option<Sender<()>>,
    proxy_kill_rx: Option<Receiver<()>>,
    /// used to reconstruct the server instance from the database
    /// this field MUST be synced to the main object
    instance_config: InstanceConfig,
}

impl ServerInstance {
    pub fn new(config: &InstanceConfig, path: PathBuf) -> ServerInstance {
        let mut jvm_args: Vec<String> = vec![];
        let config_override = config.fill_default();
        // this unwrap is safe because we just filled it in
        jvm_args.push(format!("-Xms{}M", config_override.min_ram.unwrap()));
        jvm_args.push(format!("-Xmx{}M", config_override.max_ram.unwrap()));
        jvm_args.push("-jar".to_string());
        jvm_args.push("server.jar".to_string());
        jvm_args.push("nogui".to_string());
        println!("jvm_args: {:?}", jvm_args);

        let properties_manager = PropertiesManager::new(path.join("server.properties")).unwrap();

        let event_processor = Arc::new(Mutex::new(EventProcessor::new()));
        let macro_manager = Arc::new(Mutex::new(MacroManager::new(
            path.join("macros/"),
            Arc::new(Mutex::new(None)),
            event_processor.clone(),
        )));

        let (proxy_kill_tx, proxy_kill_rx): (Sender<()>, Receiver<()>) = bounded(0);

        if let Some(true) = config.start_on_connection {
            let listener =
                TcpListener::bind(format!("127.0.0.1:{}", config.port.unwrap())).unwrap();
            listener.set_nonblocking(true).unwrap();
            let uuid = config.uuid.clone().unwrap();
            let proxy_kill_rx = proxy_kill_rx.clone();
            thread::spawn(move || {
                let mut kill = false;
                while !kill {
                    if let Ok(_) = proxy_kill_rx.try_recv() {
                        println!("Proxy kill received");
                        kill = true;
                        break;
                    }
                    if let Ok((stream, _)) = listener.accept() {
                        // drop(stream);
                        break;
                    }
                }
                drop(listener);
                drop(proxy_kill_rx);
                println!("thread exiting");
                if !kill {
                    println!("got tcp connection");
                    // loop {
                    //     thread::sleep(Duration::from_millis(1000));
                    //     println!("block {}", uuid);
                    // }
                    reqwest::blocking::Client::new()
                        .post(format!(
                            "http://127.0.0.1:8001/api/v1/instance/asd-1804a3cf626-50/start"
                        ))
                        .send()
                        .unwrap();
                    println!("end")
                }
            });
        }

        // serilize config_override to a file
        let mut file = File::create(path.join(".lodestone_config")).unwrap();
        let config_override_string = serde_json::to_string_pretty(&config_override).unwrap();
        file.write_all(config_override_string.as_bytes()).unwrap();

        let mut server_instance = ServerInstance {
            status: Arc::new(Mutex::new(Status::Stopped)),
            flavour: config.flavour,
            name: config.name.clone(),
            stdin: Arc::new(Mutex::new(None)),
            jvm_args,
            process: None,
            path: path.clone(),
            port: config.port.expect("no port provided"),
            uuid: config.uuid.as_ref().unwrap().clone(),
            player_online: Arc::new(Mutex::new(vec![])),
            event_processor,
            proxy_kill_tx: if let Some(true) = config.start_on_connection {
                Some(proxy_kill_tx)
            } else {
                None
            },
            properties_manager,
            macro_manager,
            proxy_kill_rx: if let Some(true) = config.start_on_connection {
                Some(proxy_kill_rx)
            } else {
                None
            },
            version: config_override.version.clone(),
            min_ram: config_override.min_ram.unwrap(),
            max_ram: config_override.max_ram.unwrap(),
            creation_time: config_override.creation_time.unwrap(),
            auto_start: Arc::new(Mutex::new(config_override.auto_start.unwrap())),
            restart_on_crash: Arc::new(Mutex::new(config_override.restart_on_crash.unwrap())),
            timeout_last_left: Arc::new(Mutex::new(config_override.timeout_last_left.unwrap())),
            timeout_no_activity: Arc::new(Mutex::new(config_override.timeout_no_activity.unwrap())),
            start_on_connection: Arc::new(Mutex::new(config_override.start_on_connection.unwrap())),
            instance_config: config_override,
        };

        server_instance.setup_event_processor();
        server_instance
    }

    fn setup_event_processor(&mut self) {
        let mut event_processor = self.event_processor.lock().unwrap();
        let player_online = self.player_online.clone();
        event_processor.on_player_joined(Arc::new(move |player| {
            player_online.lock().unwrap().push(player);
        }));

        let timeout_last_left = self.timeout_last_left.clone();
        let player_online = self.player_online.clone();
        let status = self.status.clone();
        let stdin = self.stdin.clone();
        event_processor.on_player_left(Arc::new(move |player| {
            let timeout = timeout_last_left.lock().unwrap().to_owned();
            if timeout > 0 {
                player_online.lock().unwrap().retain(|p| p != &player);
                let mut i = timeout;
                while i > 0 {
                    thread::sleep(Duration::from_secs(1));
                    i -= 1;
                    if player_online.lock().unwrap().len() > 0
                        || status.lock().unwrap().to_owned() != Status::Running
                    {
                        i = timeout;
                        continue;
                    }
                    println!("No player on server, shutting down in {} seconds", i);
                }
                // println!("{}", Arc::strong_count(&stdin));
                stdin
                    .lock()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .write_all(b"stop\n")
                    .unwrap();
            }
        }));

        let timeout_no_activity = self.timeout_no_activity.clone();
        let player_online = self.player_online.clone();
        let status = self.status.clone();
        let stdin = self.stdin.clone();
        event_processor.on_server_startup(Arc::new(move || {
            *status.lock().unwrap() = Status::Running;
            let timeout = timeout_no_activity.lock().unwrap().to_owned();
            if timeout > 0 {
                let mut i = timeout;
                while i > 0 {
                    thread::sleep(Duration::from_secs(1));
                    i -= 1;
                    // println!("{}", status.lock().unwrap().to_owned());
                    if player_online.lock().unwrap().len() > 0
                        || status.lock().unwrap().to_owned() != Status::Running
                    {
                        i = timeout;
                        continue;
                    }
                    println!("No activity on server, shutting down in {} seconds", i);
                }

                stdin
                    .lock()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .write_all(b"stop\n")
                    .unwrap();
            }
        }));

        let player_online = self.player_online.clone();
        event_processor.on_server_shutdown(Arc::new(move || {
            player_online.lock().unwrap().clear();
        }));
        let status = self.status.clone();
        event_processor.on_server_message(Arc::new(move |msg| {
            if msg.message.contains("Stopping server") {
                *status.lock().unwrap() = Status::Stopping;
            }
        }));

        let macro_manager = self.macro_manager.clone();
        let stdin = self.stdin.clone();
        event_processor.on_chat(Arc::new(move |player, msg| {
            let macro_manager = macro_manager.clone();
            if msg.starts_with(".macro") {
                let mut macro_name = String::new();

                let mut args = msg.split_whitespace();
                // if there is a second argument
                if let Some(name) = args.nth(1) {
                    macro_name = name.to_string();
                } else {
                    stdin
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .write_all(b"say Usage: .macro [file] [args..]\n")
                        .unwrap();
                }
                // collect the string into a vec<String>
                let mut vec_string = vec![];
                for token in args {
                    vec_string.push(token.to_owned())
                }
                thread::spawn(move || {
                    macro_manager
                        .lock()
                        .unwrap()
                        .run(macro_name, vec_string, Some(player))
                        .unwrap();
                });
            }
        }));
        let start_on_connection = self.start_on_connection.clone();
        let proxy_kill_rx = self.proxy_kill_rx.clone();
        let port = self.port;
        let uuid = self.uuid.clone();
        event_processor.on_server_shutdown(Arc::new(move || {
            if start_on_connection.lock().unwrap().to_owned() {
                let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
                listener.set_nonblocking(true).unwrap();
                let mut kill = false;
                while !kill {
                    if let Ok(_) = listener.accept() {
                        println!("tcp");
                        break;
                    }
                    if let Ok(_) = proxy_kill_rx.as_ref().unwrap().try_recv() {
                        println!("kill");

                        kill = true;
                        break;
                    }
                }
                if !kill {
                    println!("got tcp connection");
                    reqwest::blocking::Client::new()
                        .post(format!(
                            "http://127.0.0.1:8001/api/v1/instance/{}/start",
                            uuid
                        ))
                        .send()
                        .unwrap();
                }
            }
        }));
    }

    pub fn start(&mut self) -> Result<(), String> {
        let status = self.status.lock().unwrap().clone();
        env::set_current_dir(&self.path).unwrap();
        if let Some(tx) = &self.proxy_kill_tx {
            tx.send(()).unwrap();
        }
        match status {
            Status::Starting => {
                return Err("cannot start, instance is already starting".to_string())
            }
            Status::Stopping => return Err("cannot start, instance is stopping".to_string()),
            Status::Running => return Err("cannot start, instance is already running".to_string()),
            _ => (),
        }
        Command::new("bash")
            .arg(&self.path.join("prelaunch.sh"))
            .output()
            .map_err(|e| println!("{}", e.to_string()));
        *self.status.lock().unwrap() = Status::Starting;
        let mut command = Command::new("java");
        command
            .args(&self.jvm_args)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped());
        match command.spawn() {
            Ok(mut proc) => {
                env::set_current_dir("../..").unwrap();
                let stdin = proc
                    .stdin
                    .take()
                    .ok_or("failed to open stdin of child process")?;
                *self.stdin.lock().unwrap() = Some(stdin);
                let stdout = proc
                    .stdout
                    .take()
                    .ok_or("failed to open stdout of child process")?;
                let reader = BufReader::new(stdout);
                self.macro_manager
                    .lock()
                    .unwrap()
                    .set_event_processor(self.event_processor.clone());
                self.macro_manager
                    .lock()
                    .unwrap()
                    .set_stdin_sender(self.stdin.clone());

                let players_closure = self.player_online.clone();
                let event_processor_closure = self.event_processor.clone();
                let status_closure = self.status.clone();
                let uuid_closure = self.uuid.clone();
                let restart_on_crash = Arc::new(self.instance_config.restart_on_crash);
                thread::spawn(move || {
                    for line_result in reader.lines() {
                        let line = line_result.unwrap();
                        println!("server said: {}", line);
                        event_processor_closure.lock().unwrap().process(&line);
                    }

                    let status = status_closure.lock().unwrap().clone();
                    players_closure.lock().unwrap().clear();
                    println!("program exiting as reader thread is terminating...");
                    match status {
                        Status::Stopping => {
                            *status_closure.lock().unwrap() = Status::Stopped;
                            println!("instance stopped properly")
                        }
                        Status::Running => {
                            *status_closure.lock().unwrap() = Status::Stopped;
                            if let Some(true) = *restart_on_crash {
                                println!("restarting instance");
                                // make a post request to localhost
                                let client = reqwest::blocking::Client::new();
                                client
                                    .post(format!(
                                        "http://localhost:8001/api/v1/instance/{}/start",
                                        uuid_closure
                                    ))
                                    .send()
                                    .unwrap();
                            }
                        }
                        Status::Starting => {
                            println!("instance crashed while attemping to start");
                        }
                        _ => {
                            println!("this is a really weird bug");
                        }
                    }
                    event_processor_closure
                        .lock()
                        .unwrap()
                        .notify_server_shutdown();
                });
                // let start_on_connection = Arc::new(self.instance_config.start_on_connection);
                // let port = self.port;
                // let proxy_kill_rx = self.proxy_kill_rx.clone();
                // let uuid_closure = self.uuid.clone();
                // if let Some(proxy_kill_rx) = proxy_kill_rx {
                //     self.event_processor
                //         .lock()
                //         .unwrap()
                //         .on_server_shutdown(Arc::new(move || {
                //             if let Some(true) = *start_on_connection {
                //                 thread::sleep(Duration::from_secs(5));
                //                 println!("listening for proxy to connect");
                //                 let listener =
                //                     TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
                //                 listener.set_nonblocking(true).unwrap();
                //                 let mut kill = false;
                //                 while !kill {
                //                     if let Ok(_) = listener.accept() {
                //                         println!("tcp");
                //                         break;
                //                     }
                //                     proxy_kill_rx.try_recv();
                //                     if let Ok(_) = proxy_kill_rx.try_recv() {
                //                         println!("kill");

                //                         kill = true;
                //                         break;
                //                     }
                //                 }
                //                 println!("what");

                //                 if !kill {
                //                     println!("got tcp connection");
                //                     reqwest::blocking::Client::new()
                //                         .post(format!(
                //                             "http://localhost:8001/api/v1/instance/{}/start",
                //                             uuid_closure
                //                         ))
                //                         .send()
                //                         .unwrap();
                //                 }
                //             }
                //         }));
                // }
                // let status_closure = self.status.clone();
                // self.event_processor
                //     .lock()
                //     .unwrap()
                //     .on_server_startup(Arc::new(move || {
                //         *status_closure.lock().unwrap() = Status::Running;
                //     }));

                // let players_closure = self.player_online.clone();
                // let status_closure = self.status.clone();
                // let timeout_last_left = self.instance_config.timeout_last_left.unwrap();
                // let uuid_closure = self.uuid.clone();
                // if self.instance_config.timeout_last_left.unwrap() > 0 {
                //     self.event_processor
                //         .lock()
                //         .unwrap()
                //         .on_player_left(Arc::new(move |player| {
                //             // remove player from players_closur
                //             players_closure.lock().unwrap().retain(|p| p != &player);
                //             let mut i = timeout_last_left;
                //             while i > 0 {
                //                 thread::sleep(Duration::from_secs(1));
                //                 i -= 1;
                //                 if players_closure.lock().unwrap().len() > 0
                //                     || status_closure.lock().unwrap().to_owned() != Status::Running
                //                 {
                //                     i = timeout_last_left;
                //                 }
                //             }
                //             reqwest::blocking::Client::new()
                //                 .post(format!(
                //                     "http://localhost:8001/api/v1/instance/{}/stop",
                //                     uuid_closure
                //                 ))
                //                 .send()
                //                 .unwrap();
                //         }));
                // }

                // let status_closure = self.status.clone();
                // let players_closure = self.player_online.clone();
                // self.event_processor
                //     .lock()
                //     .unwrap()
                //     .on_server_message(Arc::new(move |msg| {
                //         if msg.message.contains("Stopping server") {
                //             let mut status = status_closure.lock().unwrap();
                //             players_closure.lock().unwrap().clear();
                //             *status = Status::Stopping;
                //         }
                //     }));

                // let players_closure = self.player_online.clone();
                // let status_closure = self.status.clone();
                // let stdin_sender = self.stdin.clone();
                // let timeout = self.instance_config.timeout_no_activity.unwrap();
                // if timeout > 0 {
                //     thread::spawn(move || {
                //         let mut i = timeout;
                //         while i > 0 {
                //             thread::sleep(Duration::from_secs(1));
                //             i -= 1;
                //             if players_closure.lock().unwrap().len() > 0
                //                 || status_closure.lock().unwrap().to_owned() != Status::Running
                //             {
                //                 i = timeout;
                //             }
                //         }
                //         stdin_sender
                //             .unwrap()
                //             .lock()
                //             .unwrap()
                //             .write_all(b"stop\n")
                //             .unwrap();
                //     });
                // }

                // let players_closure = self.player_online.clone();
                // self.event_processor
                //     .lock()
                //     .unwrap()
                //     .on_player_joined(Arc::new(move |player| {
                //         players_closure.lock().unwrap().push(player);
                //     }));
                self.process = Some(Arc::new(Mutex::new(proc)));

                return Ok(());
            }
            Err(_) => {
                *self.status.lock().unwrap() = Status::Stopped;
                env::set_current_dir("../..").unwrap();
                return Err("failed to open child process".to_string());
            }
        };
    }

    // invokes the stopping procedure without actually sending stop command to server
    // mainly used for when a player sends a stop command
    // fn invoke_stop(server_instance : &mut Arc<Mutex<ServerInstance>>) -> Result<(), String> {
    //     let lock = server_instance.lock().unwrap();
    //     let mut status = lock.status.lock().unwrap();
    //     match *status {
    //         Status::Starting => return Err("cannot stop, instance is starting".to_string()),
    //         Status::Stopping => return Err("cannot stop, instance is already stopping".to_string()),
    //         Status::Stopped => return Err("cannot stop, instance is already stopped".to_string()),
    //         Status::Running => println!("stopping instance"),
    //     }
    //     *status = Status::Stopping;
    //     server_instance.lock().unwrap().player_online.lock().unwrap().clear();
    //     Ok(())
    // }

    pub fn stop(&mut self) -> Result<(), String> {
        println!("stop called");
        let mut status = self.status.lock().unwrap();
        match *status {
            Status::Starting => return Err("cannot stop, instance is starting".to_string()),
            Status::Stopping => return Err("cannot stop, instance is already stopping".to_string()),
            Status::Stopped => return Err("cannot stop, instance is already stopped".to_string()),
            Status::Running => println!("stopping instance"),
        }
        *status = Status::Stopping;
        self.send_stdin("stop".to_string())?;
        self.player_online.lock().unwrap().clear();
        Ok(())
    }

    pub fn send_stdin(&self, line: String) -> Result<(), String> {
        match self.stdin.lock() {
            Ok(stdin_option) => {
                if (*stdin_option).is_none() {
                    return Err("stdin is not open".to_string());
                }
                println!("writting");
                return (*stdin_option)
                    .as_ref()
                    .unwrap()
                    .write_all(format!("{}\n", line).as_bytes())
                    .map_err(|e| format!("failed to write to stdin: {}", e));
            }
            Err(_) => Err("failed to aquire lock on stdin".to_string()),
        }
    }

    pub fn get_name(&self) -> String {
        self.name.clone()
    }

    pub fn get_status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn get_process(&self) -> Option<Arc<Mutex<Child>>> {
        self.process.clone()
    }

    pub fn get_player_list(&self) -> Vec<String> {
        self.player_online.lock().unwrap().clone()
    }

    pub fn get_player_num(&self) -> u32 {
        self.player_online.lock().unwrap().len().try_into().unwrap()
    }

    pub fn get_path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn get_uuid(&self) -> String {
        self.uuid.clone()
    }

    pub fn get_flavour(&self) -> Flavour {
        self.flavour
    }
    pub fn get_port(&self) -> u32 {
        self.port
    }

    pub fn get_instance_config(&self) -> &InstanceConfig {
        &self.instance_config
    }

    /// Get the server instance's creation time.
    #[must_use]
    pub fn creation_time(&self) -> u64 {
        self.instance_config.creation_time.unwrap()
    }

    /// Get a reference to the server instance's instance config.
    #[must_use]
    pub fn instance_config(&self) -> &InstanceConfig {
        &self.instance_config
    }
}
// mod macro_code {
//     use std::{
//         collections::HashMap,
//         fs::File,
//         io::{self, BufRead, Write},
//         path::PathBuf,
//         process::ChildStdin,
//         sync::{
//             mpsc::{self},
//             Arc, Mutex,
//         },
//         thread, time,
//     };

//     use regex::Regex;

//     use rlua::{Error, Function, Lua, MultiValue};

//     use crate::event_processor::EventProcessor;

//     pub fn dispatch_macro(
//         line: &String,
//         path: PathBuf,
//         stdin_sender: Arc<Mutex<ChildStdin>>,
//         event_processor: Arc<Mutex<EventProcessor>>,
//     ) {
//         let iterator = line.split_whitespace();
//         let mut iter = 0;
//         let mut path_to_macro = path.clone();
//         let mut args = vec![];
//         for token in iterator.clone() {
//             if iter == 0 {
//                 if token != ".macro" {
//                     return;
//                 }
//             } else if iter == 1 {
//                 path_to_macro.push(token);
//                 path_to_macro.set_extension("lua");
//                 println!("path_to_macro: {}", path_to_macro.to_str().unwrap());
//                 if !path_to_macro.exists() {
//                     stdin_sender
//                         .lock()
//                         .as_mut()
//                         .unwrap()
//                         .write_all(format!("/say macro {} does no exist\n", token).as_bytes());
//                     return;
//                 }
//             } else {
//                 println!("arg: {}", token);
//                 args.push(token.to_string());
//             }
//             iter = iter + 1;
//         }
//         if iter == 1 {
//             stdin_sender
//                 .lock()
//                 .as_mut()
//                 .unwrap()
//                 .write_all("say Usage: .macro [macro file] args..\n".as_bytes());
//             return;
//         }

//         let mut program: String = String::new();

//         for line_result in io::BufReader::new(File::open(path_to_macro).unwrap()).lines() {
//             program.push_str(format!("{}\n", line_result.unwrap()).as_str());
//         }

//         Lua::new().context(move |lua_ctx| {
//             for (pos, arg) in args.iter().enumerate() {
//                 println!("setting {} to {}", format!("arg{}", pos + 1), arg);
//                 lua_ctx
//                     .globals()
//                     .set(format!("arg{}", pos + 1), arg.clone());
//             }
//             let delay_sec = lua_ctx
//                 .create_function(|_, time: u64| {
//                     thread::sleep(std::time::Duration::from_secs(time));
//                     Ok(())
//                 })
//                 .unwrap();
//             lua_ctx.globals().set("delay_sec", delay_sec);

//             let event_processor_clone = event_processor.clone();
//             let await_msg = lua_ctx
//                 .create_function(move |lua_ctx, ()| {
//                     let (tx, rx) = mpsc::channel();
//                     let index = event_processor_clone.lock().unwrap().on_chat.len();
//                     event_processor_clone.lock().unwrap().on_chat.push(Box::new(
//                         move |player, player_msg| {
//                             tx.send((player, player_msg)).unwrap();
//                         },
//                     ));
//                     println!("awaiting message");
//                     let (player, player_msg) = rx.recv().unwrap();
//                     println!("got message from {}: {}", player, player_msg);
//                     // remove the callback
//                     event_processor_clone.lock().unwrap().on_chat.remove(index);
//                     Ok((player, player_msg))
//                 })
//                 .unwrap();
//             lua_ctx.globals().set("await_msg", await_msg);
//             let delay_milli = lua_ctx
//                 .create_function(|_, time: u64| {
//                     thread::sleep(std::time::Duration::from_millis(time));
//                     Ok(())
//                 })
//                 .unwrap();
//             lua_ctx.globals().set("delay_milli", delay_milli);
//             let send_stdin = lua_ctx
//                 .create_function(move |ctx, line: String| {
//                     // println!("sending {}", line);
//                     let reg = Regex::new(r"\$\{(\w*)\}").unwrap();
//                     let globals = ctx.globals();
//                     let mut after = line.clone();
//                     if reg.is_match(&line) {
//                         for cap in reg.captures_iter(&line) {
//                             println!("cap1: {}", cap.get(1).unwrap().as_str());
//                             after = after.replace(
//                                 format!("${{{}}}", &cap[1]).as_str(),
//                                 &globals.get::<_, String>(cap[1].to_string()).unwrap(),
//                             );
//                             println!("after: {}", after);
//                         }

//                         stdin_sender
//                             .lock()
//                             .as_mut()
//                             .unwrap()
//                             .write_all(format!("{}\n", after).as_bytes());
//                     } else {
//                         stdin_sender
//                             .lock()
//                             .unwrap()
//                             .write_all(format!("{}\n", line).as_bytes());
//                     }

//                     Ok(())
//                 })
//                 .unwrap();
//             lua_ctx.globals().set("sendStdin", send_stdin);

//             lua_ctx.globals().set(
//                 "isBadWord",
//                 lua_ctx
//                     .create_function(|_, word: String| {
//                         use censor::*;
//                         let censor = Standard + "lambda";
//                         Ok((censor.check(word.as_str()),))
//                     })
//                     .unwrap(),
//             );

//             match lua_ctx.load(&program).eval::<MultiValue>() {
//                 Ok(value) => {
//                     println!(
//                         "{}",
//                         value
//                             .iter()
//                             .map(|value| format!("{:?}", value))
//                             .collect::<Vec<_>>()
//                             .join("\t")
//                     );
//                 }
//                 // Err(Error::SyntaxError {
//                 //     incomplete_input: true,
//                 //     ..
//                 // }) => {}
//                 Err(e) => {
//                     eprintln!("error: {}", e);
//                 }
//             }
//         });
//     }
// }
