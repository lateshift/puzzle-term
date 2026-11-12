//! Browser sockets use a real local PTY bridge supplied by the acceptance harness.
//! The private test URL stays in the process environment; no token enters fixtures.
use super::*;

/// A bounded wait yields to actual WebSocket events and fails with no private URL.
async fn wait(terminal: &PuzzleTerminal, predicate: impl Fn(&Runtime) -> bool) {
    let start = Instant::now();
    while !predicate(&terminal.owner.borrow()) {
        assert!(start.elapsed().as_secs() < 20, "bridge response timed out");
        delay(5).await;
    }
}

/// A browser task boundary lets parser continuation, socket messages and rAF run.
pub(super) async fn delay(ms: i32) {
    let promise = Promise::new(&mut |resolve, _| {
        window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .unwrap();
    });
    JsFuture::from(promise).await.unwrap();
}

/// Hello, encoded stdin, output, resize and drained exit cross the production client.
#[wasm_bindgen_test(async)]
async fn real_bridge_hello_echo_resize_exit() {
    let url = option_env!("PUZZLE_TEST_BRIDGE_URL")
        .expect("real bridge URL missing; run cargo xtask web test");
    let _fonts = resources::Fonts::new();
    let (terminal, host) = resources::create(14).await;
    let options = js_sys::Object::new();
    js_sys::Reflect::set(&options, &"protocol".into(), &"puzzle-bridge-v1".into()).unwrap();
    let connection = terminal.connect(url.into(), Some(options.into())).unwrap();
    wait(&terminal, |r| {
        r.socket
            .as_ref()
            .is_some_and(|s| s.ws.ready_state() == web_sys::WebSocket::OPEN)
    })
    .await;
    terminal
        .input("printf '\\033]2;echo-ready\\007'\r".into())
        .unwrap();
    wait(&terminal, |r| r.session.buffer().title() == "echo-ready").await;
    terminal.resize(100.into(), 30.into()).unwrap();
    // The resize control cannot wait for input credit, and stty reads the child PTY.
    terminal
        .input("stty size; printf '\\033]2;size-ready\\007'\r".into())
        .unwrap();
    wait(&terminal, |r| r.session.buffer().title() == "size-ready").await;
    let contents = {
        let r = terminal.owner.borrow();
        let buffer = r.session.buffer();
        buffer.serialize(
            buffer.line_ids().unwrap(),
            puzzle_term::vt::view::Format::PlainText,
        )
    };
    assert!(
        contents.contains("30 100"),
        "stty must observe the resized grid"
    );
    terminal
        .input("printf 'final-output'; exit 3\r".into())
        .unwrap();
    wait(&terminal, |r| r.session.exit_info().is_some()).await;
    assert_eq!(
        terminal.owner.borrow().session.exit_info().unwrap().code,
        Some(3)
    );
    assert_eq!(terminal.pending_bytes().unwrap(), 0.0);
    connection.disconnect();
    terminal.dispose().unwrap();
    host.remove();
}

/// The client decodes literal binary grants and controls in wasm, not just native code.
#[wasm_bindgen_test]
fn codec_round_trip_in_wasm() {
    use puzzle_term::bridge::{self, Client, Server};
    let mut bytes = Vec::new();
    bridge::encode_server(Server::InputCredit(1_048_576), &mut bytes).unwrap();
    assert_eq!(bytes, [0x84, 0, 0, 16, 0]);
    assert_eq!(
        bridge::decode_server(&bytes).unwrap(),
        Server::InputCredit(1_048_576)
    );
    bridge::encode_client(Client::Ack(65_536), &mut bytes).unwrap();
    assert_eq!(bridge::decode_client(&bytes).unwrap(), Client::Ack(65_536));
}

/// Raw sockets accept text/binary output and send unframed input without bridge controls.
#[wasm_bindgen_test(async)]
async fn raw_socket_text_binary_and_unframed_input() {
    let _fonts = resources::Fonts::new();
    let (terminal, host) = resources::create(14).await;
    let url = option_env!("PUZZLE_TEST_BRIDGE_URL").expect("run cargo xtask web test");
    let connection = terminal.connect(url.into(), None).unwrap();
    wait(&terminal, |r| {
        r.socket
            .as_ref()
            .is_some_and(|s| s.ws.ready_state() == web_sys::WebSocket::OPEN)
    })
    .await;
    let socket = terminal.owner.borrow().socket.as_ref().unwrap().ws.clone();
    let received = Rc::new(RefCell::new(Vec::<u8>::new()));
    let sink = received.clone();
    let send = Closure::wrap(Box::new(move |bytes: js_sys::Uint8Array| {
        sink.borrow_mut().extend(bytes.to_vec())
    }) as Box<dyn FnMut(js_sys::Uint8Array)>);
    // Capture the browser send boundary while retaining the real socket event listeners.
    js_sys::Reflect::set(socket.as_ref(), &"send".into(), send.as_ref()).unwrap();
    for binary in [false, true] {
        let init = web_sys::MessageEventInit::new();
        let bytes = b"\x1b]2;raw-output\x07";
        let data: JsValue = if binary {
            js_sys::Uint8Array::from(bytes.as_slice()).buffer().into()
        } else {
            "\x1b]2;raw-output\x07".into()
        };
        init.set_data(&data);
        socket
            .dispatch_event(
                &web_sys::MessageEvent::new_with_event_init_dict("message", &init).unwrap(),
            )
            .unwrap();
        assert_eq!(
            terminal.owner.borrow().session.buffer().title(),
            "raw-output"
        );
    }
    terminal.input("a".into()).unwrap();
    terminal.resize(100.into(), 30.into()).unwrap();
    assert_eq!(
        received.borrow().as_slice(),
        b"a",
        "raw endpoints get no resize/ACK/hello protocol"
    );
    connection.disconnect();
    terminal.dispose().unwrap();
    host.remove();
}
