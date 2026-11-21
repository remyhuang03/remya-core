//! Per-output brightness through Wayland gamma ramps, separate from night light.
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Seek, Write},
    os::fd::AsFd,
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, delegate_noop,
    protocol::{wl_callback, wl_output, wl_registry},
};
use wayland_protocols_wlr::gamma_control::v1::client::{
    zwlr_gamma_control_manager_v1, zwlr_gamma_control_v1,
};

pub const TOPIC: &str = "display-gamma";
pub const OUTPUT_PREFIX: &str = "display-gamma:";
const TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct SoftwareDimming {
    active: BTreeMap<String, Session>,
    lost: BTreeMap<String, String>,
}

impl SoftwareDimming {
    pub fn get(&mut self, output: &str) -> Value {
        let result = match self.active.get_mut(output) {
            Some(session) => session.pump(Duration::ZERO).map(|()| session.percent),
            None => Session::open(output).and_then(|session| session.release(false)),
        };
        if let Err(reason) = &result
            && self.active.remove(output).is_some()
        {
            self.lost.insert(output.to_owned(), reason.clone());
        }
        response(output, result)
    }

    pub fn set(&mut self, output: &str, raw: &str) -> Result<Value, String> {
        let percent = raw
            .parse()
            .ok()
            .filter(|value| (10..=100).contains(value))
            .ok_or("InvalidArgument:Gamma must be 10 to 100")?;
        self.lost.remove(output);
        let mut previous = self.active.remove(output);
        if previous
            .as_mut()
            .is_some_and(|session| session.pump(Duration::ZERO).is_err())
        {
            previous = None;
        }
        let result = (|| {
            let session = previous.map_or_else(|| Session::open(output), Ok)?;
            if percent == 100 {
                let was_dimmed = session.percent < 100;
                return session.release(was_dimmed);
            }
            let mut session = session;
            session.apply(percent)?;
            self.active.insert(output.to_owned(), session);
            Ok(percent)
        })();
        Ok(response(output, result))
    }

    pub fn changes(&mut self) -> Vec<Value> {
        self.active.retain(|output, session| {
            if let Err(reason) = session.pump(Duration::ZERO) {
                self.lost.insert(output.clone(), reason);
                return false;
            }
            true
        });
        std::mem::take(&mut self.lost)
            .into_iter()
            .map(|(output, reason)| response(&output, Err(reason)))
            .collect()
    }
}

fn response(output: &str, result: Result<u32, String>) -> Value {
    let (gamma, available, reason) = match result {
        Ok(percent) => (percent, true, String::new()),
        Err(reason) => (100, false, reason),
    };
    json!({"output": output, "gamma": gamma, "available": available, "reason": reason})
}

struct Output {
    proxy: wl_output::WlOutput,
    name: Option<String>,
}

#[derive(Default)]
struct Registry {
    manager: Option<zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1>,
    outputs: BTreeMap<u32, Output>,
    selected: Option<u32>,
    size: Option<u32>,
    failure: Option<&'static str>,
    synced: bool,
}

struct Session {
    connection: Connection,
    queue: EventQueue<Registry>,
    registry: Registry,
    control: zwlr_gamma_control_v1::ZwlrGammaControlV1,
    percent: u32,
}

impl Session {
    fn open(output: &str) -> Result<Self, String> {
        let connection = Connection::connect_to_env()
            .map_err(|error| format!("Could not connect to Wayland: {error}"))?;
        let mut queue = connection.new_event_queue();
        let handle = queue.handle();
        connection.display().get_registry(&handle, ());
        let mut registry = Registry::default();
        let deadline = Instant::now() + TIMEOUT;
        sync(&connection, &mut queue, &mut registry, deadline)?;
        // wl_output.name is sent after the registry handler binds each wl_output.
        sync(&connection, &mut queue, &mut registry, deadline)?;
        let (&id, output) = registry
            .outputs
            .iter()
            .find(|(_, candidate)| candidate.name.as_deref() == Some(output))
            .ok_or("Wayland output is unavailable")?;
        let manager = registry
            .manager
            .as_ref()
            .ok_or("The compositor does not provide software dimming")?;
        let control = manager.get_gamma_control(&output.proxy, &handle, ());
        registry.selected = Some(id);
        sync(&connection, &mut queue, &mut registry, deadline)?;
        if !registry
            .size
            .is_some_and(|size| (2..=65536).contains(&size))
        {
            return Err("The output did not provide a usable gamma ramp".into());
        }
        Ok(Self {
            connection,
            queue,
            registry,
            control,
            percent: 100,
        })
    }

    fn apply(&mut self, percent: u32) -> Result<(), String> {
        if percent == self.percent {
            return Ok(());
        }
        let size = self.registry.size.ok_or("No gamma ramp size")?;
        let mut file = tempfile::tempfile()
            .map_err(|error| format!("Could not prepare gamma ramp: {error}"))?;
        file.write_all(&ramp(size, percent))
            .and_then(|()| file.rewind())
            .map_err(|error| format!("Could not prepare gamma ramp: {error}"))?;
        self.control.set_gamma(file.as_fd());
        sync(
            &self.connection,
            &mut self.queue,
            &mut self.registry,
            Instant::now() + TIMEOUT,
        )?;
        self.percent = percent;
        Ok(())
    }

    fn release(mut self, restore: bool) -> Result<u32, String> {
        if restore {
            // Installing identity schedules a frame before destroying the control.
            self.apply(100)?;
        }
        self.control.destroy();
        sync(
            &self.connection,
            &mut self.queue,
            &mut self.registry,
            Instant::now() + TIMEOUT,
        )?;
        Ok(100)
    }

    fn pump(&mut self, timeout: Duration) -> Result<(), String> {
        pump(&mut self.queue, &mut self.registry, timeout)
    }
}

fn ramp(size: u32, percent: u32) -> Vec<u8> {
    let divisor = u64::from(size - 1) * 100;
    let channel: Vec<_> = (0..size)
        .flat_map(|index| {
            let value = u64::from(index) * u64::from(u16::MAX) * u64::from(percent);
            (((value + divisor / 2) / divisor) as u16).to_ne_bytes()
        })
        .collect();
    channel.repeat(3)
}

fn sync(
    connection: &Connection,
    queue: &mut EventQueue<Registry>,
    registry: &mut Registry,
    deadline: Instant,
) -> Result<(), String> {
    registry.synced = false;
    connection.display().sync(&queue.handle(), ());
    while !registry.synced {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Wayland did not respond in time".into());
        }
        pump(queue, registry, remaining)?;
    }
    Ok(())
}

fn pump(
    queue: &mut EventQueue<Registry>,
    registry: &mut Registry,
    timeout: Duration,
) -> Result<(), String> {
    fn wayland_error(error: impl std::fmt::Display) -> String {
        format!("Wayland dimming disconnected: {error}")
    }
    if queue.dispatch_pending(registry).map_err(wayland_error)? == 0 {
        queue.flush().map_err(wayland_error)?;
        if let Some(guard) = queue.prepare_read() {
            let mut fds = [PollFd::from_borrowed_fd(
                guard.connection_fd(),
                PollFlags::IN,
            )];
            let wait = Timespec {
                tv_sec: timeout.as_secs() as _,
                tv_nsec: timeout.subsec_nanos().into(),
            };
            match poll(&mut fds, Some(&wait)) {
                Ok(0) | Err(rustix::io::Errno::INTR) => {}
                Ok(_) => {
                    guard.read().map_err(wayland_error)?;
                    queue.dispatch_pending(registry).map_err(wayland_error)?;
                }
                Err(error) => return Err(wayland_error(error)),
            }
        }
    }
    registry.failure.map_or(Ok(()), |reason| Err(reason.into()))
}

impl Dispatch<wl_registry::WlRegistry, ()> for Registry {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == "wl_output" && version >= 4 => {
                let proxy = registry.bind(name, 4, handle, name);
                state.outputs.insert(name, Output { proxy, name: None });
            }
            wl_registry::Event::Global {
                name, interface, ..
            } if interface == "zwlr_gamma_control_manager_v1" => {
                state.manager = Some(registry.bind(name, 1, handle, ()));
            }
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(output) = state.outputs.remove(&name) {
                    output.proxy.release();
                }
                if state.selected == Some(name) {
                    state.failure = Some("The Wayland output disconnected");
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, u32> for Registry {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event
            && let Some(output) = state.outputs.get_mut(id)
        {
            output.name = Some(name);
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for Registry {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.synced = true;
        }
    }
}

impl Dispatch<zwlr_gamma_control_v1::ZwlrGammaControlV1, ()> for Registry {
    fn event(
        state: &mut Self,
        _: &zwlr_gamma_control_v1::ZwlrGammaControlV1,
        event: zwlr_gamma_control_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_gamma_control_v1::Event::GammaSize { size } => state.size = Some(size),
            zwlr_gamma_control_v1::Event::Failed => {
                state.failure = Some("The compositor rejected gamma control for this output");
            }
            _ => {}
        }
    }
}

delegate_noop!(Registry: ignore zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_is_planar_neutral_monotonic_and_keeps_black() {
        let values = ramp(1024, 25);
        let (red, others) = values.split_at(2048);
        assert_eq!(others, red.repeat(2));
        let ramp: Vec<_> = red
            .as_chunks::<2>()
            .0
            .iter()
            .map(|value| u16::from_ne_bytes(*value))
            .collect();
        assert_eq!(ramp[0], 0);
        assert_eq!(ramp[1023], 16384);
        assert!(ramp.is_sorted());
    }

    #[test]
    fn failed_output_reports_reset_without_global_fallback() {
        assert_eq!(
            response("DP-1", Err("disconnected".into())),
            json!({"output": "DP-1", "gamma": 100, "available": false, "reason": "disconnected"})
        );
        assert!(SoftwareDimming::default().set("DP-1", "101").is_err());
    }
}
