#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::{
    doc::{NodeId, ParagraphFormat, TextList},
    Session,
};
use serde_json::json;

fn formats(s: &Session, id: NodeId) -> Vec<ParagraphFormat> {
    let d = s.doc().get(id).unwrap().text().unwrap();
    (0..d.paragraph_count())
        .map(|i| d.paragraph_format(i))
        .collect()
}

fn item(list: TextList, level: u8) -> ParagraphFormat {
    ParagraphFormat { list, level }
}

#[test]
fn paragraphs_take_their_own_lists_through_edits_and_history() {
    let mut s = Session::new().unwrap();
    let id = NodeId(
        s.dispatch(
            "node.create.text",
            &json!({"x": 0, "y": 0, "content": "Steps\nOne\nTwo\nNote"}),
        )
        .unwrap()["id"]
            .as_u64()
            .unwrap(),
    );
    let plain = ParagraphFormat::default();
    s.dispatch(
        "text.setParagraphs",
        &json!({"id": id, "first": 1, "last": 2, "list": "number"}),
    )
    .unwrap();
    s.dispatch(
        "text.setParagraphs",
        &json!({"id": id, "first": 2, "last": 2, "indent": 1}),
    )
    .unwrap();
    assert_eq!(
        formats(&s, id),
        [
            plain,
            item(TextList::Number, 0),
            item(TextList::Number, 1),
            plain
        ]
    );
    s.dispatch(
        "text.setContent",
        &json!({"id": id, "content": "Steps\nOne\nTwo\nThree\nNote"}),
    )
    .unwrap();
    assert_eq!(formats(&s, id)[3], item(TextList::Number, 1));
    assert_eq!(formats(&s, id)[4], plain);
    s.dispatch(
        "text.setParagraphs",
        &json!({"id": id, "first": 3, "last": 3, "indent": -5}),
    )
    .unwrap();
    assert_eq!(formats(&s, id)[3], item(TextList::Number, 0));
    assert!(s
        .dispatch(
            "text.setParagraphs",
            &json!({"id": id, "first": 2, "last": 9})
        )
        .is_err());
    assert!(s
        .dispatch("text.setParagraphs", &json!({"id": id, "level": 9}))
        .is_err());
    s.dispatch("text.setStyle", &json!({"ids": [id], "list": "bullet"}))
        .unwrap();
    assert_eq!(formats(&s, id)[2], item(TextList::Bullet, 1));
    s.dispatch("edit.undo", &json!({})).unwrap();
    assert_eq!(formats(&s, id)[0], plain);
    s.dispatch(
        "text.setParagraphs",
        &json!({"id": id, "list": "none", "level": 0}),
    )
    .unwrap();
    let d = s.doc().get(id).unwrap().text().unwrap();
    assert!(d.paragraphs.is_empty());
    assert_eq!(d.list, TextList::None);
}
