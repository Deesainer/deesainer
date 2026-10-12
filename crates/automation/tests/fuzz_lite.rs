#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_automation::control;
use deesainer_engine::Session;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn random_protocol_lines_never_panic_or_corrupt() {
    let mut s = Session::new().unwrap();
    let commands: Vec<String> = s.registry().iter().map(|c| c.id.to_owned()).collect();
    let values = [
        "0",
        "1",
        "-1",
        "1e308",
        "\"x\"",
        "null",
        "true",
        "[]",
        "{}",
        "[1,2]",
        "\"#ff0000\"",
        "3",
    ];
    let keys = [
        "id",
        "ids",
        "x",
        "y",
        "width",
        "height",
        "parent",
        "fill",
        "name",
        "text",
        "content",
        "data",
        "to",
        "component",
        "direction",
    ];
    let mut rng = Lcg(99);
    for _ in 0..3000 {
        let cmd = &commands[(rng.next() as usize) % commands.len()];
        let mut params = String::from("{");
        for k in 0..(rng.next() % 4) {
            if k > 0 {
                params.push(',');
            }
            params.push_str(&format!(
                "\"{}\":{}",
                keys[(rng.next() as usize) % keys.len()],
                values[(rng.next() as usize) % values.len()]
            ));
        }
        params.push('}');
        let line = format!("{{\"id\":1,\"method\":\"engine.execute\",\"params\":{{\"command\":\"{cmd}\",\"params\":{params}}}}}");
        let reply = control::handle_line(&mut s, &line);
        assert!(reply.get("result").is_some() || reply.get("error").is_some());
        s.doc().validate().unwrap();
    }
}
