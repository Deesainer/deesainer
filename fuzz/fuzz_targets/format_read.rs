#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = deesainer_format::read(data) {
        assert!(doc.validate().is_ok());
    }
});
