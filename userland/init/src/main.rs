#![no_std]
#![no_main]

#[cfg(not(test))]
use core::panic::PanicInfo;
use init::{
    MAX_AFTER, MAX_SERVICES, SERVICE_TOML, ServiceManifest, parse_services, topological_sort,
};
use libturnix::{exec, exit, fork, kill, print, read_shutdown_signal, shutdown, wait};

const SIGTERM: u8 = 15;
const SIGKILL: u8 = 9;

// ── Print helpers ────────────────────────────────────────────────────────
fn print_u64(mut n: u64) {
    if n == 0 {
        print("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut idx = 20;
    while n > 0 {
        idx -= 1;
        buf[idx] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if let Ok(s) = core::str::from_utf8(&buf[idx..]) {
        print(s);
    }
}

fn println(s: &str) {
    print(s);
    print("\n");
}

// ── Entry point ──────────────────────────────────────────────────────────
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    println("turnix Init Daemon v4");
    println("[BOOT OK]");

    let mut services = [ServiceManifest {
        name: "",
        path: "",
        after: [""; MAX_AFTER],
        after_count: 0,
        pid: 0,
    }; MAX_SERVICES];

    let count = parse_services(SERVICE_TOML, &mut services);
    if count == 0 {
        println("No services defined — entering wait loop");
    } else {
        print("Parsed ");
        print_u64(count as u64);
        println(" service(s)");
    }

    if count > 0 && !topological_sort(&mut services, count) {
        println("ERROR: Cycle detected in service dependencies — starting anyway");
    }

    for i in 0..count {
        let svc = &services[i];
        print("Starting ");
        print(svc.name);
        print(" (");
        print(svc.path);
        print(")");
        if svc.after_count > 0 {
            print(" after [");
            for d in 0..svc.after_count {
                if d > 0 {
                    print(", ");
                }
                print(svc.after[d]);
            }
            print("]");
        }
        println("");

        let pid = fork();
        if pid == 0 {
            let path = svc.path;
            let argv: [*const u8; 2] = [path.as_ptr(), core::ptr::null()];
            let envp: [*const u8; 1] = [core::ptr::null()];
            exec(path, argv.as_ptr(), envp.as_ptr());
            #[allow(unreachable_code)]
            {
                print("FAILED to exec ");
                print(path);
                println("");
                exit(1);
            }
        } else if (pid as i64) < 0 {
            print("FAILED to fork for ");
            print(svc.name);
            println("");
        } else {
            services[i].pid = pid;
            print("  -> PID ");
            print_u64(pid);
            println("");
        }
    }

    println("Entering reaper loop");

    let mut exited_count = 0;

    loop {
        let mut status: i32 = 0;
        let child_pid = wait(&mut status as *mut i32);

        if (child_pid as i64) > 0 {
            let mut svc_name = "unknown";
            for i in 0..count {
                if services[i].pid == child_pid {
                    svc_name = services[i].name;
                    services[i].pid = 0;
                    exited_count += 1;
                    break;
                }
            }
            print("Reaped ");
            print(svc_name);
            print(" (PID ");
            print_u64(child_pid);
            print("), status=");
            print_u64(status as u64);
            println("");
        }

        if read_shutdown_signal() != 0 {
            println("Shutdown signal received — stopping services");
            break;
        }

        if count > 0 && exited_count >= count {
            println("All services exited — still reaping orphans");
        }
    }

    shutdown_services(&mut services, count);

    println("Calling shutdown");
    shutdown();
}

fn shutdown_services(services: &mut [ServiceManifest], count: usize) {
    for i in (0..count).rev() {
        if services[i].pid == 0 {
            continue;
        }
        let pid = services[i].pid as i32;
        print("Sending SIGTERM to ");
        print(services[i].name);
        print(" (PID ");
        print_u64(pid as u64);
        println(")");
        kill(pid, SIGTERM);
    }

    loop {
        let mut status: i32 = 0;
        let pid = wait(&mut status as *mut i32);
        if (pid as i64) <= 0 {
            break;
        }
        for i in 0..count {
            if services[i].pid == pid {
                services[i].pid = 0;
                break;
            }
        }
    }

    for i in (0..count).rev() {
        if services[i].pid == 0 {
            continue;
        }
        let pid = services[i].pid as i32;
        print("Sending SIGKILL to ");
        print(services[i].name);
        println("");
        kill(pid, SIGKILL);
    }

    loop {
        let mut status: i32 = 0;
        let pid = wait(&mut status as *mut i32);
        if (pid as i64) <= 0 {
            break;
        }
    }
}

// ── Standard no_std cruft ────────────────────────────────────────────────
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub extern "C" fn mainCRTStartup() {}
