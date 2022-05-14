use tokio::{process::{Command}, io::AsyncReadExt};
use tokio::io::{BufReader, AsyncBufReadExt};
use std::{process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy)]
enum Msg {
    Login,
    KexecLoad,
    KexecBoot,
    Reboot
}

#[derive(Debug, Clone, Copy)]
enum State {
    Startup,
    KexecLoad,
    KexecBoot,
    Startup2,
    Reboot,
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {

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
    
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let (tx, mut rx) = mpsc::channel(1024);

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match msg {
                Msg::Login => {
                    stdin.write(b"root").await.unwrap();
                }
                Msg::KexecLoad => {
                    stdin.write(br#"kexec -l /boot/bzImage --append="root=/dev/vda1 console=ttyS0 nokaslr ignore_loglevel debug" --initrd=/boot/rootfs.ext4 --mem-min=0x10000000"#).await.unwrap();
                }
                Msg::KexecBoot => {
                    stdin.write(b"kexec -e").await.unwrap();
                }
                Msg::Reboot => {
                    stdin.write(b"reboot").await.unwrap();
                }
            }
            stdin.write(b"\n").await.unwrap();
            stdin.flush().await.unwrap();
        }
    });

    tokio::spawn(async move {
        let mut state = State::Startup;
        let mut line = String::new();
        let mut buf = [0;1];
        loop {
            if let 1 = stdout.read(&mut buf).await.unwrap() {
                match buf[0] as char {
                    '\n' => {
                        println!("{}", &line);
                        line.clear();
                    }
                    '\r' => (),
                    c => {
                        line.push(c);
                        match (state, &*line) {
                            (State::Startup, "buildroot login: ") => {
                                state = State::KexecLoad;
                                tx.send(Msg::Login).await.unwrap();
                            }
                            (State::KexecLoad, "# ") => {
                                state = State::KexecBoot;
                                tx.send(Msg::KexecLoad).await.unwrap();
                            }
                            (State::KexecBoot, "# ") => {
                                state = State::Startup2;
                                tx.send(Msg::KexecBoot).await.unwrap();
                            }
                            (State::Startup2, "buildroot login: ") => {
                                state = State::Reboot;
                                tx.send(Msg::Login).await.unwrap();
                            }
                            (State::Reboot, "# ") => {
                                tx.send(Msg::Reboot).await.unwrap();
                            }
                            _ => (),
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
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>  {
    run().await
}
