//! The browser's end of the live feed: a WebSocket to the device that
//! served the page. The device forwards the CAN bus as slcan text, the same
//! lines a CANable prints, so the conversation is the shared `Session`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::sync::Mutex;

use eframe::wasm_bindgen::closure::Closure;
use eframe::wasm_bindgen::JsCast;
use web_time::{Duration, Instant};

use crate::slcan::Session;
use crate::snapshot::Snapshot;
use crate::source::Stepper;
use crate::stored::Ask;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Default)]
struct Inbox {
    bytes: Vec<u8>,
    opened: bool,
    closed: Option<String>,
}

pub struct Socket {
    url: String,
    ws: Option<web_sys::WebSocket>,
    inbox: Rc<RefCell<Inbox>>,
    ask: Receiver<Ask>,
    session: Session,
    started: Instant,
    // The closures must live as long as the socket.
    _hold: Vec<Closure<dyn FnMut(web_sys::Event)>>,
    _hold_message: Option<Closure<dyn FnMut(web_sys::MessageEvent)>>,
}

impl Socket {
    pub fn new(url: &str, ask: Receiver<Ask>) -> Self {
        Self {
            url: url.to_string(),
            ws: None,
            inbox: Rc::new(RefCell::new(Inbox::default())),
            ask,
            session: Session::new(),
            started: Instant::now(),
            _hold: Vec::new(),
            _hold_message: None,
        }
    }
}

impl Stepper for Socket {
    fn begin(&mut self, _snapshot: &Mutex<Snapshot>) -> Result<(), String> {
        let ws = web_sys::WebSocket::new(&self.url)
            .map_err(|_| format!("Could not open a connection to {}", self.url))?;
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let inbox = Rc::clone(&self.inbox);
        let on_open = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            inbox.borrow_mut().opened = true;
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));

        let inbox = Rc::clone(&self.inbox);
        let url = self.url.clone();
        let on_close = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            inbox.borrow_mut().closed = Some(format!("The connection to {url} closed"));
        });
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        let inbox = Rc::clone(&self.inbox);
        let url = self.url.clone();
        let on_error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let mut inbox = inbox.borrow_mut();
            if inbox.closed.is_none() {
                inbox.closed = Some(format!("Could not connect to {url}"));
            }
        });
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        let inbox = Rc::clone(&self.inbox);
        let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let data = event.data();
            let mut inbox = inbox.borrow_mut();
            if let Some(text) = data.as_string() {
                inbox.bytes.extend_from_slice(text.as_bytes());
            } else if data.is_instance_of::<js_sys::ArrayBuffer>() {
                inbox.bytes.extend_from_slice(&js_sys::Uint8Array::new(&data).to_vec());
            }
        });
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        self._hold = vec![on_open, on_close, on_error];
        self._hold_message = Some(on_message);
        self.ws = Some(ws);
        self.started = Instant::now();
        Ok(())
    }

    fn tick(&mut self, snapshot: &Mutex<Snapshot>) -> Result<(), String> {
        let (bytes, opened, closed) = {
            let mut inbox = self.inbox.borrow_mut();
            (std::mem::take(&mut inbox.bytes), inbox.opened, inbox.closed.clone())
        };
        let mut out = Vec::new();
        if opened {
            self.session.poll(snapshot, &self.ask, &mut out);
        } else if closed.is_none() && self.started.elapsed() > CONNECT_TIMEOUT {
            return Err(format!("Could not connect to {} (no answer)", self.url));
        }
        for byte in bytes {
            self.session.on_byte(byte, snapshot, &mut out);
        }
        if let (Some(ws), true) = (self.ws.as_ref(), opened) {
            for line in out {
                ws.send_with_str(&line)
                    .map_err(|_| "Could not send to the device".to_string())?;
            }
        }
        match closed {
            Some(message) => Err(message),
            None => Ok(()),
        }
    }

    fn interval(&self) -> Duration {
        Duration::from_millis(10)
    }

    fn end(&mut self, snapshot: &Mutex<Snapshot>) {
        if let Some(ws) = self.ws.take() {
            // Debug off on disconnect. The
            // browser sends what is queued before it closes the socket.
            let mut out = Vec::new();
            self.session.disconnecting(snapshot, &mut out);
            for line in out {
                let _ = ws.send_with_str(&line);
            }
            ws.set_onopen(None);
            ws.set_onclose(None);
            ws.set_onerror(None);
            ws.set_onmessage(None);
            let _ = ws.close();
        }
    }
}
