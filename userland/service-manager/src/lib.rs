//! Service manager for Turnix OS
//! Manages system services and daemons

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy)]
pub enum ServiceStatus {
    Stopped,
    Running,
    Failed,
}

pub struct Service {
    pub name: String,
    pub description: String,
    pub status: ServiceStatus,
    pub auto_start: bool,
}

pub struct ServiceManager {
    services: Vec<Service>,
}

impl ServiceManager {
    pub fn new() -> Self {
        Self {
            services: Vec::new(),
        }
    }
    
    pub fn register(&mut self, service: Service) {
        crate::println!("[SRV] Registering service: {}", service.name);
        self.services.push(service);
    }
    
    pub fn start(&mut self, name: &str) -> bool {
        crate::println!("[SRV] Starting service: {}", name);
        for service in &mut self.services {
            if service.name == name {
                service.status = ServiceStatus::Running;
                crate::println!("[SRV] Service {} started", name);
                return true;
            }
        }
        crate::println!("[SRV] Service {} not found", name);
        false
    }
    
    pub fn stop(&mut self, name: &str) -> bool {
        crate::println!("[SRV] Stopping service: {}", name);
        for service in &mut self.services {
            if service.name == name {
                service.status = ServiceStatus::Stopped;
                crate::println!("[SRV] Service {} stopped", name);
                return true;
            }
        }
        false
    }
    
    pub fn list(&self) {
        crate::println!("[SRV] Services:");
        for service in &self.services {
            let status_str = match service.status {
                ServiceStatus::Stopped => "stopped",
                ServiceStatus::Running => "running",
                ServiceStatus::Failed => "failed",
            };
            crate::println!("  - {}: {} ({})", service.name, service.description, status_str);
        }
    }
    
    pub fn start_all(&mut self) {
        crate::println!("[SRV] Starting all auto-start services...");
        for service in &mut self.services {
            if service.auto_start {
                service.status = ServiceStatus::Running;
                crate::println!("  Started: {}", service.name);
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn main() {
    crate::println!("[SRV] Turnix Service Manager v0.1.0");
    
    let mut sm = ServiceManager::new();
    
    // Register example services
    sm.register(Service {
        name: "network".into(),
        description: "Network daemon".into(),
        status: ServiceStatus::Stopped,
        auto_start: true,
    });
    
    sm.register(Service {
        name: "logger".into(),
        description: "Logging daemon".into(),
        status: ServiceStatus::Stopped,
        auto_start: true,
    });
    
    // Start all auto-start services
    sm.start_all();
    sm.list();
}
