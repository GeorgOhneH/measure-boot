use plot::plot;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::ChildStdin;
use tokio::time::{Duration, Instant};
use tokio::{io::AsyncReadExt, process::Command};

mod plot;

#[derive(Debug)]
pub struct Data {
    normal_start: StartData,
    kexec_load_time: Duration,
    kexec_start: StartData,
}

#[derive(Debug)]
pub struct StartData {
    entry: Duration,
    run_init: Duration,
    login: Duration,
}

impl StartData {
    pub fn new(entry: Duration, run_init: Duration, login: Duration) -> Self {
        Self {
            entry,
            run_init,
            login,
        }
    }
}

impl From<TimeStamps> for Data {
    fn from(stamps: TimeStamps) -> Self {
        let kexec_load_finished = stamps.kexec_load_finished.unwrap();
        Self {
            normal_start: StartData::new(
                stamps.kernel_start.unwrap(),
                stamps.run_init.unwrap(),
                stamps.login.unwrap(),
            ),
            kexec_start: StartData::new(
                stamps.kexec_kernel_start.unwrap() - kexec_load_finished,
                stamps.kexec_run_init.unwrap() - kexec_load_finished,
                stamps.kexec_login.unwrap() - kexec_load_finished,
            ),
            kexec_load_time: kexec_load_finished - stamps.kexec_load.unwrap(),
        }
    }
}

#[derive(Debug)]
struct TimeStamps {
    pub kernel_start: Option<Duration>,
    pub run_init: Option<Duration>,
    pub login: Option<Duration>,
    pub kexec_load: Option<Duration>,
    pub kexec_load_finished: Option<Duration>,
    pub kexec_kernel_start: Option<Duration>,
    pub kexec_run_init: Option<Duration>,
    pub kexec_login: Option<Duration>,

    start: Instant,
}

impl TimeStamps {
    pub fn new() -> Self {
        Self {
            kernel_start: None,
            run_init: None,
            login: None,
            kexec_load: None,
            kexec_load_finished: None,
            kexec_kernel_start: None,
            kexec_run_init: None,
            kexec_login: None,
            start: Instant::now(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum State {
    Startup,
    KexecLoad,
    KexecBoot,
    Startup2,
    Reboot,
    Finished,
}

async fn write_line(stdin: &mut ChildStdin, text: &[u8]) {
    stdin.write(text).await.unwrap();
    stdin.write(b"\n").await.unwrap();
    stdin.flush().await.unwrap();
}

async fn new_char(
    state: State,
    line: &str,
    stdin: &mut ChildStdin,
    timestamp: &mut TimeStamps,
) -> State {
    match (state, line) {
        (State::Startup, "buildroot login: ") => {
            let elapsed = timestamp.start.elapsed();
            timestamp.login = Some(elapsed);
            write_line(stdin, b"root").await;
            State::KexecLoad
        }
        (State::KexecLoad, "# ") => {
            let elapsed = timestamp.start.elapsed();
            timestamp.kexec_load = Some(elapsed);
            write_line(stdin, br#"kexec -l /boot/bzImage --append="root=/dev/vda1 console=ttyS0 nokaslr ignore_loglevel debug" --initrd=/boot/rootfs.ext4 --mem-min=0x10000000"#).await;
            State::KexecBoot
        }
        (State::KexecBoot, "# ") => {
            let elapsed = timestamp.start.elapsed();
            timestamp.kexec_load_finished = Some(elapsed);
            write_line(stdin, b"kexec -e").await;
            State::Startup2
        }
        (State::Startup2, "buildroot login: ") => {
            let elapsed = timestamp.start.elapsed();
            timestamp.kexec_login = Some(elapsed);
            write_line(stdin, b"root").await;
            State::Reboot
        }
        (State::Reboot, "# ") => {
            write_line(stdin, b"reboot").await;
            State::Finished
        }
        _ => state,
    }
}

async fn run() -> Result<Data, Box<dyn std::error::Error>> {
    println!("Hello, world!");
    let mut child = Command::new("./qemu/bin/x86_64-softmmu/qemu-system-x86_64")
        .args(["-m" ,"4G,slots=2,maxmem=8G"])
        .arg("-nographic")
        .arg("-no-reboot")
        .args(["-object" ,"memory-backend-file,id=mem1,share=on,mem-path=/mnt/my-pmem/startup-ram,size=4G,readonly=off"])
        .args(["-device" ,"nvdimm,id=nvdimm1,memdev=mem1,unarmed=off"])
        .args(["-machine" ,"pc,nvdimm=on"])
        .arg("-enable-kvm")
        .args(["-cpu" ,"host"])
        .args(["-smp" ,"cores=24"])
        .args(["-drive" ,"file=buildroot-2022.02/output/images/disk.img,if=virtio,format=raw"])
        .args(["-smp" ,"cores=24"])
        .args(["-net" ,"nic,model=virtio"])
        .args(["-net" ,"user"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir("..")
        .spawn()
        .expect("failed to start process");

    let mut timestamp = TimeStamps::new();

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let handle = tokio::spawn(async move {
        let mut state = State::Startup;
        let mut line = String::new();
        let mut buf = [0; 1];
        loop {
            if let 1 = stdout.read(&mut buf).await.unwrap() {
                match buf[0] as char {
                    '\n' => {
                        println!("{}", &line);
                        if line.contains("Run /sbin/init as init process") {
                            let time: f64 = line[1..13].trim().parse().unwrap();
                            let elapsed = timestamp.start.elapsed();
                            match state {
                                State::Startup => {
                                    timestamp.run_init = Some(elapsed);
                                    timestamp.kernel_start =
                                        Some(elapsed - Duration::from_secs_f64(time));
                                }
                                State::Startup2 => {
                                    timestamp.kexec_run_init = Some(elapsed);
                                    timestamp.kexec_kernel_start =
                                        Some(elapsed - Duration::from_secs_f64(time));
                                }
                                _ => unimplemented!(),
                            }
                        }

                        line.clear();
                    }
                    '\r' => (),
                    c => {
                        line.push(c);
                        state = new_char(state, &line, &mut stdin, &mut timestamp).await;
                        if let State::Finished = state {
                            return timestamp;
                        }
                    }
                }
            }
        }
    });

    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Some(line) = reader.next_line().await.unwrap() {
            println!("{}", line);
        }
    });

    let status = child.wait().await?;

    println!("the command exited with: {}", status);

    handle.abort();
    Ok(handle.await.unwrap().into())
}

#[tokio::main]
async fn main() {
    let mut data = Vec::new();
    for _ in 0..5 {
        data.push(run().await.unwrap())
    }
    dbg!(&data);
    plot(data).unwrap();
}
