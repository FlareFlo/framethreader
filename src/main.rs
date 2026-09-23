use std::env;
use std::env::VarError;
use std::process::Command;

const LAUNCH_ENV_FLAG: &str = "FRAMETHREADER_RUNMODE";

pub fn main() {
    let mode = env::var(&LAUNCH_ENV_FLAG);
    let self_path = env::current_exe().unwrap();

    let mut children = vec![];
    match mode.as_deref() {
        Ok(wgpu_renderer::RUNMODE) => {
            wgpu_renderer::realmain();
        }
        Ok(launcher::RUNMODE) => {
            launcher::realmain();
        }
        Err(VarError::NotPresent) => {
            let child = Command::new(self_path.as_os_str()).env(LAUNCH_ENV_FLAG, launcher::RUNMODE).spawn().unwrap();
            children.push(child);
        }
        _ => {
            launcher::realmain();
        }
    }

    let mut happy = None;
    for child in &mut children {
        let exit = child.wait().unwrap();
        if !exit.success() {
            happy = Some(exit);
            break;
        }
    }

    if let Some(status) = happy {
        eprintln!("Child exited with {status}. Killing rest");
        for mut child in children {
            child.kill().unwrap();
        }
        todo!("Spawn crash handler here");
    }
}
