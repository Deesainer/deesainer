#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let (Ok(text), Ok(mut session)) = (std::str::from_utf8(data), deesainer_engine::Session::new()) {
        let _ = deesainer_automation::control::handle_line(&mut session, text);
        assert!(session.doc().validate().is_ok());
    }
});
