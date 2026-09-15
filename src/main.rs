use std::env;

const LAUNCH_ENV_FLAG: &str = "FRAMETHREADER_RUNMODE";


pub fn main() {
	let mode = env::var(&LAUNCH_ENV_FLAG);

	match mode.as_deref() {
		Ok(wgpu_renderer::RUNMODE) => {
			wgpu_renderer::realmain();
		}
		Ok(launcher::RUNMODE) | _ => {
			launcher::realmain();
		}
	}
}
