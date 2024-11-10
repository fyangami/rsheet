use rsheet_lib::command::Command;
use rsheet_lib::connect::{
    Connection, Manager, ReadMessageResult, Reader, WriteMessageResult, Writer,
};
use rsheet_lib::replies::Reply;
use sheet::Sheet;
use std::collections::HashSet;
use std::error::Error;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

mod sheet;

pub fn start_server<M>(mut manager: M) -> Result<(), Box<dyn Error>>
where
    M: Manager,
{
    // This initiates a single client connection, and reads and writes messages
    // indefinitely.

    let sht = Arc::new(RwLock::new(Sheet::new()));
    let id_sets: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    loop {
        let (mut recv, mut send) = match manager.accept_new_connection() {
            Connection::NewConnection { reader, writer } => (reader, writer),
            Connection::NoMoreConnections => {
                // There are no more new connections to accept.
                return Ok(());
            }
        };
        let id_sets_cloned = id_sets.clone();
        let mut id_sets_guard = id_sets_cloned.lock().expect("lock error");
        if id_sets_guard.contains(&send.id()) {
            // simply ignore duplicated connections.
            continue;
        }
        let sht = sht.clone();
        id_sets_guard.insert(send.id().to_string());
        drop(id_sets_guard);
        let id_sets_cloned = id_sets.clone();
        thread::spawn(move || {
            loop {
                match recv.read_message() {
                    ReadMessageResult::Message(msg) => {
                        let reply = match msg.parse::<Command>() {
                            Ok(command) => match command {
                                Command::Get { cell_identifier } => match sht.read() {
                                    Ok(sht) => sht.get(&cell_identifier),
                                    Err(e) => Reply::Error(e.to_string()),
                                },
                                Command::Set {
                                    cell_identifier,
                                    cell_expr,
                                } => {
                                    let mut sht_guard = sht.write().expect("lock error");
                                    let reply = sht_guard.set(&cell_identifier, cell_expr);
                                    // TODO update dependencies.
                                    reply
                                }
                            },
                            Err(e) => Reply::Error(e.to_string()),
                        };
                        match send.write_message(reply) {
                            WriteMessageResult::Ok => {
                                // Message successfully sent, continue.
                            }
                            WriteMessageResult::ConnectionClosed => {
                                // The connection was closed. This is not an error, but
                                // should terminate this connection.
                                break;
                            }
                            WriteMessageResult::Err(_) => {
                                // An unexpected error was encountered.
                                break;
                            }
                        };
                    }
                    ReadMessageResult::ConnectionClosed => {
                        // The connection was closed. This is not an error, but
                        // should terminate this connection.
                        break;
                    }
                    ReadMessageResult::Err(_) => {
                        // An unexpected error was encountered.
                        break;
                    }
                }
            }
            let mut id_sets_guard = id_sets_cloned.lock().expect("lock error");
            id_sets_guard.remove(&send.id());
        });
    }
}
