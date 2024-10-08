use afrish::*;

fn main() {
    let root = afrish::start_wish().unwrap();
    root.title("font-example-1.rs");

    let label_1 = afrish::make_label(&root);
    label_1.text("Default font");
    label_1.grid().row(0).layout();

    let label_2 = afrish::make_label(&root);
    label_2.text("font: helvetica 12 bold");
    label_2.font(&afrish::TkFont {
        family: String::from("Helvetica"),
        size: 12,
        weight: afrish::Weight::Bold,
        ..Default::default()
    });
    label_2.grid().row(1).layout();

    let label_3 = afrish::make_label(&root);
    label_3.text("font: courier 8");
    label_3.font(&afrish::TkFont {
        family: String::from("Courier"),
        size: 8,
        ..Default::default()
    });
    label_3.grid().row(2).layout();

    afrish::mainloop();
}

