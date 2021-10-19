use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path;
use std::process::Command;

use glob::glob;
use nix::unistd;
use yaml_rust::YamlLoader;

macro_rules! copy {
    ($copy:stmt) => {
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
                    "error creating dir {} from {}",
                    &destination, &path
                ));
                fs::set_permissions(&destination, metadata.permissions()).expect(&*format!(
                    "error copying mode {} from {}",
                    &destination, &path
                ));
            } else $copy
        });
    };
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let preflight_file: &str;

    if args.len() < 2 {
        preflight_file = "preflight.yaml";
    } else {
        preflight_file = &args[1];
    }

    println!("[preflight] Starting {}", preflight_file);

    let preflight_contents = fs::read_to_string(preflight_file).expect(&*format!(
        "Failed to read preflight file {}",
        preflight_file
    ));

    let docs = YamlLoader::load_from_str(&*preflight_contents).expect(&*format!(
        "Failed to load preflight file {}",
        preflight_file
    ));

    let preflight = &docs[0]; // we only care about first document

    let mut c = 0;
    for task in preflight["tasks"].clone().into_iter() {
        println!(
            "[preflight task #{}] {}",
            c,
            task["message"].as_str().unwrap()
        );
        match task["action"].as_str().unwrap() {
            "copy" => {
                copy! {{
                    fs::copy(path, &destination).expect(&*format!(
                        "error copying file {} from {}",
                        &destination, &path
                    ));
                }}
            }
            "copy-if-absent" => {
                copy! {{
                    if !path::Path::new(&destination).exists() {
                        fs::copy(path, &destination).expect(&*format!(
                            "error copying file {} from {}",
                            &destination, &path
                        ));
                    }
                }}
            }
            "move" => {
                let from = task["from"].as_str().unwrap();
                let to = task["to"].as_str().unwrap();
                fs::rename(from, to).expect(&*format!("error moving file {} to {}", &from, &to));
            }
            "chown" => {
                let target = task["target"].as_str().unwrap();
                let uid = unistd::Uid::from_raw(task["uid"].as_i64().unwrap() as u32);
                let gid = unistd::Gid::from_raw(task["gid"].as_i64().unwrap() as u32);
                let root = get_pre_glob_path(target);
                unistd::chown(root.as_str(), Some(uid), Some(gid)).expect(&*format!(
                    "error chowning file {} to {}:{}",
                    &root, uid, gid
                ));
                glob_walk_exec(target, |path| {
                    unistd::chown(path, Some(uid), Some(gid)).expect(&*format!(
                        "error chowning file {} to {}:{}",
                        &path, uid, gid
                    ));
                })
            }
            "chmod" => {
                let target = task["target"].as_str().unwrap();
                // mode: "value" # Must be quoted or it's interpreted as a int and as_str() is unwrap()'d to None.
                let mode = u32::from_str_radix(task["mode"].as_str().unwrap(), 8).unwrap();
                let root = get_pre_glob_path(target);
                fs::set_permissions(root.as_str(), fs::Permissions::from_mode(mode))
                    .expect(&*format!("error setting mode {} to {}", &root, mode));
                glob_walk_exec(target, |path| {
                    fs::set_permissions(path, fs::Permissions::from_mode(mode))
                        .expect(&*format!("error setting mode {} to {}", &path, mode));
                })
            }
            "mkfile" => {
                let target = task["target"].as_str().unwrap();
                if !path::Path::new(target).exists() {
                    fs::File::create(target).expect(&*format!("error creating file {}", target));
                }
            }
            "mkdir" => {
                let target = task["target"].as_str().unwrap();
                fs::create_dir_all(target).expect(&*format!("error creating dir {}", target));
            }
            _ => {
                println!("invalid task action...")
            }
        };
        c += 1;
    }

    match preflight["keep"].as_bool() {
        None | Some(false) => {
            println!("[preflight] Cleaning up...");
            fs::remove_file(&args[0]).expect(&*format!("Error deleting self at {}", args[0]));
            fs::remove_file(preflight_file)
                .expect(&*format!("Error deleting config at {}", args[0]));
        }
        Some(true) => {}
    };

    let mut exec_argv = vec![];
    for arg in preflight["argv"].as_vec().unwrap() {
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
