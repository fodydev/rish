use afrish::*;

fn main() {
    let root = afrish::start_wish().unwrap();

    root.title("entry-example.rs");

    let entry_1 = afrish::make_entry(&root);
    let entry_2 = afrish::make_entry(&root);
    entry_2.show('*');

    let name_label = afrish::make_label(&root);
    name_label.text("Name:");
    name_label.grid().row(0).column(0).layout();
    entry_1.grid().row(0).column(1).pady(5).layout();

    let password_label = afrish::make_label(&root);
    password_label.text("Password:");
    password_label.grid().row(1).column(0).layout();
    entry_2.grid().row(1).column(1).pady(5).layout();

    let show_button = afrish::make_button(&root);
    show_button.text("Show entries");
    show_button.command(move || {
        println!("{} - {}", entry_1.value_get(), entry_2.value_get());
    });
    show_button.grid().row(2).column(0).column_span(2).layout();

    afrish::mainloop();
}
