use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path;
use std::process::Command;

use glob::glob;
use nix::unistd;
use toml::Value;

fn main() {
    let args: Vec<String> = env::args().collect();

    let preflight_file: &str;

    if args.len() < 2 {
        preflight_file = "preflight.toml";
    } else {
        preflight_file = &args[1];
    }

    println!("[preflight] Starting {}", preflight_file);

    let preflight_contents = fs::read_to_string(preflight_file).expect(&*format!(
        "Failed to read preflight file {}",
        preflight_file
    ));

    let docs = preflight_contents.parse::<Value>().expect(&*format!(
        "Failed to load preflight file {}",
        preflight_file
    ));

    let preflight = &docs["preflight"]; // we only care about first document

    let mut c = 0;
    for task in preflight["tasks"].as_array().expect("[preflight] No tasks loaded") {
        println!(
            "[preflight task #{}] {}",
            c,
            task["message"].as_str().unwrap()
        );
        let action = task["action"].as_str().unwrap();
        match action {
            "copy" | "copy-if-absent" => {
                let from = task["from"].as_str().unwrap();
                let to = task["to"].as_str().unwrap();
                let prefix = get_pre_glob_path(from);
                glob_walk_exec(from, |path| {
                    let destination;
                    let metadata = fs::metadata(path).unwrap();
                    let s = path.trim_start_matches(&prefix);
                    if s.len() > 0 {
                        destination = vec![to, s].join("/");
                    } else {
                        destination = to.to_string();
                    }

                    if metadata.is_dir() {
                        fs::create_dir_all(&destination).expect(&*format!(
                            "Error creating dir {} from {}",
                            &destination, &path
                        ));
                        fs::set_permissions(&destination, metadata.permissions()).expect(
                            &*format!("Error copying mode {} from {}", &destination, &path),
                        );
                    } else {
                        if action == "copy" {
                            fs::copy(path, &destination).expect(&*format!(
                                "Error copying file {} from {}",
                                &destination, &path
                            ));
                        } else {
                            if !path::Path::new(&destination).exists() {
                                fs::copy(path, &destination).expect(&*format!(
                                    "Error copying file {} from {}",
                                    &destination, &path
                                ));
                            }
                        }
                    }
                });
            }
            "move" => {
                let from = task["from"].as_str().unwrap();
                let to = task["to"].as_str().unwrap();
                fs::rename(from, to).expect(&*format!("Error moving file {} to {}", &from, &to));
            }
            "chown" => {
                let target = task["target"].as_str().unwrap();

                let uid = match task.get("uid") {
                    Some(Value::Integer(v)) => {
                        Some(unistd::Uid::from_raw(*v as u32))
                    }
                    Some(Value::String(v)) =>  {
                        let id = u32::from_str_radix(v, 10).expect("Invalid uid string");
                        Some(unistd::Uid::from_raw(id))
                    }
                    _ => {
                        None
                    }
                };

                let gid = match task.get("gid") {
                    Some(Value::Integer(v)) => {
                        Some(unistd::Gid::from_raw(*v as u32))
                    }
                    Some(Value::String(v)) =>  {
                        let id = u32::from_str_radix(v, 10).expect("Invalid gid string");
                        Some(unistd::Gid::from_raw(id))
                    }
                    _ => {
                        None
                    }
                };
                let root = get_pre_glob_path(target);
                // Probably ends up in a double walk
                unistd::chown(root.as_str(), uid, gid).expect(&*format!(
                    "Error chowning file {} to {}:{}",
                    &root, uid.unwrap_or(unistd::Uid::from_raw(0)), gid.unwrap_or(unistd::Gid::from_raw(0))
                ));
                glob_walk_exec(target, |path| {
                    unistd::chown(path, uid, gid).expect(&*format!(
                        "Error chowning file {} to {}:{}",
                        &path, uid.unwrap_or(unistd::Uid::from_raw(0)), gid.unwrap_or(unistd::Gid::from_raw(0))
                    ));
                })
            }
            "chmod" => {
                let target = task["target"].as_str().unwrap();
                // mode: "value" # Must be quoted or it's interpreted as a int and as_str() is unwrap()'d to None.
                let mode = u32::from_str_radix(task["mode"].as_str().unwrap(), 8).unwrap();
                let root = get_pre_glob_path(target);
                // Probably ends up in a double walk
                fs::set_permissions(root.as_str(), fs::Permissions::from_mode(mode))
                    .expect(&*format!("Error setting mode {} to {}", &root, mode));
                glob_walk_exec(target, |path| {
                    fs::set_permissions(path, fs::Permissions::from_mode(mode))
                        .expect(&*format!("Error setting mode {} to {}", &path, mode));
                })
            }
            "mkfile" => {
                let target = task["target"].as_str().unwrap();
                if !path::Path::new(target).exists() {
                    fs::File::create(target).expect(&*format!("Error creating file {}", target));
                }
            }
            "mkdir" => {
                let target = task["target"].as_str().unwrap();
                fs::create_dir_all(target).expect(&*format!("Error creating dir {}", target));
            }
            _ => {
                println!("Invalid task action...")
            }
        };
        c += 1;
    }

    match preflight.get("keep") {
        Some(v) => {
            match v.as_bool() {
                None | Some(false) => {
                    cleanup(&args, preflight_file)
                }
                Some(true) => {}
            };
        }
        None => {
            cleanup(&args, preflight_file)
        }
    };

    let mut exec_argv = vec![];
    for arg in preflight["argv"].as_array().expect("Missing argv") {
        exec_argv.push(arg.as_str().unwrap());
    }

    println!("[preflight] {}", preflight["message"].as_str().unwrap());
    Command::new(preflight["exec"].as_str().unwrap())
        .args(exec_argv)
        .exec();
}

fn glob_walk_exec<F>(path: &str, f: F)
where
    F: Fn(&str),
{
    for entry in glob(path).expect("Invalid glob pattern") {
        match entry {
            Ok(path) => f(&path.to_str().unwrap()),
            Err(e) => println!("{:?}", e),
        }
    }
}

// Guess I'm just only using wildcard globs
fn get_pre_glob_path(path: &str) -> String {
    let mut result = Vec::new();
    let paths = path.split("/");
    for path in paths {
        if path.contains("*") {
            break;
        }

        result.push(path);
    }

    result.join("/")
}

fn cleanup(args: &Vec<String>, preflight_file: &str) {
    println!("[preflight] Cleaning up {}", &args[0]);
    fs::remove_file(&args[0]).expect(&*format!("Error deleting self at {}", args[0]));
    println!("[preflight] Cleaning up {}", preflight_file);
    fs::remove_file(preflight_file)
        .expect(&*format!("Error deleting config at {}", args[0]));
}
