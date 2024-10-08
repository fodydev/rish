//! Core functions and data structures for interacting with the wish process.
//!
//! The basic structure of a program using wish is as follows:
//!
//! ```ignore
//! fn main() {
//!   let root = afrish::start_wish().unwrap();
//!
//!   // -- add code here to create program
//!
//!   afrish::mainloop();
//! }
//! ```
//!
//! The call to `start_wish` starts the "wish" program and sets up some
//! internal structure to store information about your program's interaction
//! with wish. The return value is a `Result`, so must be unwrapped (or
//! otherwise handled) to obtain the top-level window.
//!
//! If you are using a different program to "wish", e.g. a tclkit, then
//! call instead:
//!
//! ```ignore
//!   let root = afrish::start_with("tclkit").unwrap();
//! ```
//!
//! All construction of the GUI must be done after starting a wish process.
//!
//! (For debugging purposes, [trace_with] additionally displays all
//! messages to/from the wish program on stdout.)
//!
//! Tk is event-driven, so the code sets up the content and design
//! of various widgets and associates commands to particular events: events
//! can be button-clicks or the movement of a mouse onto a canvas.
//!
//! Once the GUI is created, then the [mainloop] must be started, which will
//! process and react to events: the call to `mainloop` is usually the last
//! statement in the program.
//!
//! The program will usually exit when the top-level window is closed. However,
//! that can be over-ridden or, to exit in another way, use [end_wish].
//!
//! ## Low-level API
//!
//! The modules in this crate aim to provide a rust-friendly, type-checked set
//! of structs and methods for using the Tk library.
//!
//! However, there are many features in Tk and not all of them are likely to be
//! wrapped. If there is a feature missing they may be used by directly calling
//! Tk commands through the low-level API.
//!
//! 1. every widget has an `id` field, which gives the Tk identifier.
//! 2. [tell_wish] sends a given string directly to wish
//! 3. [ask_wish] sends a given string directly to wish and
//!    returns, as a [String], the response.
//!
//! For example, label's
//! [takefocus](https://www.tcl-lang.org/man/tcl8.6/TkCmd/ttk_widget.htm#M-takefocus)
//! flag is not wrapped. You can nevertheless set its value using:
//!
//! ```ignore
//! let label = afrish::make_label(&root);
//!
//! afrish::tell_wish(&format!("{} configure -takefocus 0", &label.id));
//! ```
//!
//! Also useful are:
//!
//! * [cget](widget::TkWidget::cget) - queries any option and returns its current value
//! * [configure](widget::TkWidget::configure) - used to set any option to a value
//! * [winfo](widget::TkWidget::winfo) - returns window-related information
//!
//! ## Extensions
//!
//! Extensions can be created with the help of [next_wid],
//! which returns a new, unique ID in Tk format. Writing an extension requires:
//!
//! 1. importing the tcl/tk library (using `tell_wish`)
//! 2. creating an instance of the underlying Tk widget using a unique id
//! 3. retaining that id in a struct, for later reference
//! 4. wrapping the widget's functions as methods, calling out to Tk with
//!    the stored id as a reference.
//!

use std::collections::HashMap;
use std::io::{Read, Write};
use std::process;
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::thread;

use super::font;
use super::toplevel;
use super::widget;

/// Reports an error in interacting with the Tk program.
#[derive(Debug)]
pub struct TkError {
    #[allow(dead_code)]
    message: String,
}

static TRACE_WISH: OnceLock<bool> = OnceLock::new();
fn tracing() -> bool {
    *TRACE_WISH.get().unwrap_or(&false)
}

static mut WISH: OnceLock<process::Child> = OnceLock::new();
static mut OUTPUT: OnceLock<process::ChildStdout> = OnceLock::new();
static mut SENDER: OnceLock<mpsc::Sender<String>> = OnceLock::new();

// Kills the wish process - should be called to exit
pub(super) fn kill_wish() {
    unsafe {
        WISH.get_mut()
            .unwrap()
            .kill()
            .expect("Wish was unexpectedly already finished");
    }
}

/// Sends a message (tcl command) to wish.
///
/// Use with caution: the message must be valid tcl.
///
pub fn tell_wish(msg: &str) {
    if tracing() {
        println!("wish: {}", msg);
    }
    unsafe {
        SENDER.get_mut().unwrap().send(String::from(msg)).unwrap();
        SENDER.get_mut().unwrap().send(String::from("\n")).unwrap();
    }
}

/// Sends a message (tcl command) to wish and expects a result.
/// Returns a result as a string
///
/// Use with caution: the message must be valid tcl.
///
pub fn ask_wish(msg: &str) -> String {
    tell_wish(msg);

    unsafe {
        let mut input = [32; 10000]; // TODO - long inputs can get split?
        if OUTPUT.get_mut().unwrap().read(&mut input).is_ok() {
            if let Ok(input) = String::from_utf8(input.to_vec()) {
                if tracing() {
                    println!("---: {:?}", input.trim());
                }
                return input.trim().to_string();
            }
        }
    }

    panic!("Eval-wish failed to get a result");
}

// -- Counter for making new ids

fn next_static_id() -> &'static Mutex<i64> {
    static NEXT_ID: OnceLock<Mutex<i64>> = OnceLock::new();

    NEXT_ID.get_or_init(|| Mutex::new(0))
}

/// Returns a new id string which can be used to name a new
/// widget instance. The new id will be in reference to the
/// parent, as is usual in Tk.
///
/// This is only for use when writing an extension library.
///
pub fn next_wid(parent: &str) -> String {
    let mut nid = next_static_id().lock().unwrap();
    *nid += 1;
    if parent == "." {
        format!(".r{}", nid)
    } else {
        format!("{}.r{}", parent, nid)
    }
}

/// Returns a new variable name.
///
/// This is only for use when writing an extension library.
///
pub fn next_var() -> String {
    let mut nid = next_static_id().lock().unwrap();
    *nid += 1;
    format!("::var{}", nid)
}

pub(super) fn current_id() -> i64 {
    let nid = next_static_id().lock().unwrap();
    *nid
}

// -- Store for callback functions, such as on button clicks

type Callback0 = Box<(dyn Fn() + Send + 'static)>;
pub(super) fn mk_callback0<F>(f: F) -> Callback0
where
    F: Fn() + Send + 'static,
{
    Box::new(f)
}

fn static_callbacks0() -> &'static Mutex<HashMap<String, Callback0>> {
    static CALLBACKS0: OnceLock<Mutex<HashMap<String, Callback0>>> = OnceLock::new();

    CALLBACKS0.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn add_callback0(wid: &str, callback: Callback0) {
    static_callbacks0()
        .lock()
        .unwrap()
        .insert(String::from(wid), callback);
}

fn get_callback0(wid: &str) -> Option<Callback0> {
    if let Some((_, command)) = static_callbacks0().lock().unwrap().remove_entry(wid) {
        Some(command)
    } else {
        None
    }
}

fn eval_callback0(wid: &str) {
    if let Some(command) = get_callback0(wid) {
        command();
        if !wid.contains("after") && // after commands apply once only
            !static_callbacks0().lock().unwrap().contains_key(wid)
        // do not overwrite if a replacement command added
        {
            add_callback0(wid, command);
        }
    } // TODO - error?
}

type Callback1Bool = Box<(dyn Fn(bool) + Send + 'static)>;
pub(super) fn mk_callback1_bool<F>(f: F) -> Callback1Bool
where
    F: Fn(bool) + Send + 'static,
{
    Box::new(f)
}

fn static_callbacks1bool() -> &'static Mutex<HashMap<String, Callback1Bool>> {
    static CALLBACKS1BOOL: OnceLock<Mutex<HashMap<String, Callback1Bool>>> = OnceLock::new();

    CALLBACKS1BOOL.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn add_callback1_bool(wid: &str, callback: Callback1Bool) {
    static_callbacks1bool()
        .lock()
        .unwrap()
        .insert(String::from(wid), callback);
}

fn get_callback1_bool(wid: &str) -> Option<Callback1Bool> {
    if let Some((_, command)) = static_callbacks1bool().lock().unwrap().remove_entry(wid) {
        Some(command)
    } else {
        None
    }
}

fn eval_callback1_bool(wid: &str, value: bool) {
    if let Some(command) = get_callback1_bool(wid) {
        command(value);
        if !static_callbacks1bool().lock().unwrap().contains_key(wid) {
            add_callback1_bool(wid, command);
        }
    } // TODO - error?
}

type Callback1Event = Box<(dyn Fn(widget::TkEvent) + Send + 'static)>;
pub(super) fn mk_callback1_event<F>(f: F) -> Callback1Event
where
    F: Fn(widget::TkEvent) + Send + 'static,
{
    Box::new(f)
}

// for bound events, key is widgetid/all + pattern, as multiple events can be
// bound to same entity
fn static_callbacks1event() -> &'static Mutex<HashMap<String, Callback1Event>> {
    static CALLBACKS1EVENT: OnceLock<Mutex<HashMap<String, Callback1Event>>> = OnceLock::new();

    CALLBACKS1EVENT.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn add_callback1_event(wid: &str, callback: Callback1Event) {
    static_callbacks1event()
        .lock()
        .unwrap()
        .insert(String::from(wid), callback);
}

fn get_callback1_event(wid: &str) -> Option<Callback1Event> {
    if let Some((_, command)) = static_callbacks1event().lock().unwrap().remove_entry(wid) {
        Some(command)
    } else {
        None
    }
}

fn eval_callback1_event(wid: &str, value: widget::TkEvent) {
    if let Some(command) = get_callback1_event(wid) {
        command(value);
        if !static_callbacks1event().lock().unwrap().contains_key(wid) {
            add_callback1_event(wid, command);
        }
    } // TODO - error?
}

type Callback1Float = Box<(dyn Fn(f64) + Send + 'static)>;
pub(super) fn mk_callback1_float<F>(f: F) -> Callback1Float
where
    F: Fn(f64) + Send + 'static,
{
    Box::new(f)
}

fn static_callbacks1float() -> &'static Mutex<HashMap<String, Callback1Float>> {
    static CALLBACKS1FLOAT: OnceLock<Mutex<HashMap<String, Callback1Float>>> = OnceLock::new();

    CALLBACKS1FLOAT.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn add_callback1_float(wid: &str, callback: Callback1Float) {
    static_callbacks1float()
        .lock()
        .unwrap()
        .insert(String::from(wid), callback);
}

fn get_callback1_float(wid: &str) -> Option<Callback1Float> {
    if let Some((_, command)) = static_callbacks1float().lock().unwrap().remove_entry(wid) {
        Some(command)
    } else {
        None
    }
}

fn eval_callback1_float(wid: &str, value: f64) {
    if let Some(command) = get_callback1_float(wid) {
        command(value);
        if !static_callbacks1float().lock().unwrap().contains_key(wid) {
            add_callback1_float(wid, command);
        }
    } // TODO - error?
}

type Callback1Font = Box<(dyn Fn(font::TkFont) + Send + 'static)>;
pub(super) fn mk_callback1_font<F>(f: F) -> Callback1Font
where
    F: Fn(font::TkFont) + Send + 'static,
{
    Box::new(f)
}

fn static_callbacks1font() -> &'static Mutex<HashMap<String, Callback1Font>> {
    static CALLBACKS1FONT: OnceLock<Mutex<HashMap<String, Callback1Font>>> = OnceLock::new();

    CALLBACKS1FONT.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn add_callback1_font(wid: &str, callback: Callback1Font) {
    static_callbacks1font()
        .lock()
        .unwrap()
        .insert(String::from(wid), callback);
}

fn get_callback1_font(wid: &str) -> Option<Callback1Font> {
    if let Some((_, command)) = static_callbacks1font().lock().unwrap().remove_entry(wid) {
        Some(command)
    } else {
        None
    }
}

fn eval_callback1_font(wid: &str, value: font::TkFont) {
    if let Some(command) = get_callback1_font(wid) {
        command(value);
        if !static_callbacks1font().lock().unwrap().contains_key(wid) {
            add_callback1_font(wid, command);
        }
    } // TODO - error?
}

/// Loops while GUI events occur
pub fn mainloop() {
    unsafe {
        loop {
            let mut input = [32; 10000];
            if OUTPUT.get_mut().unwrap().read(&mut input).is_ok() {
                if let Ok(input) = String::from_utf8(input.to_vec()) {
                    if tracing() {
                        println!("Callback: {:?}", &input.trim());
                    }

                    // here - do a match or similar on what was read from wish
                    if input.starts_with("clicked") {
                        // -- callbacks
                        if let Some(n) = input.find(['\n', '\r']) {
                            let widget = &input[8..n];
                            eval_callback0(widget);
                        }
                    } else if input.starts_with("cb1b") {
                        // -- callback 1 with bool
                        let parts: Vec<&str> = input.split('-').collect();
                        let widget = parts[1].trim();
                        let value = parts[2].trim();
                        eval_callback1_bool(widget, value == "1");
                    } else if input.starts_with("cb1e") {
                        // -- callback 1 with event
                        let parts: Vec<&str> = input.split(':').collect();
                        let widget_pattern = parts[1].trim();
                        let x = parts[2].parse::<i64>().unwrap_or(0);
                        let y = parts[3].parse::<i64>().unwrap_or(0);
                        let root_x = parts[4].parse::<i64>().unwrap_or(0);
                        let root_y = parts[5].parse::<i64>().unwrap_or(0);
                        let height = parts[6].parse::<i64>().unwrap_or(0);
                        let width = parts[7].parse::<i64>().unwrap_or(0);
                        let key_code = parts[8].parse::<u64>().unwrap_or(0);
                        let key_symbol = parts[9].parse::<String>().unwrap_or_default();
                        let mouse_button = parts[10].parse::<u64>().unwrap_or(0);
                        let event = widget::TkEvent {
                            x,
                            y,
                            root_x,
                            root_y,
                            height,
                            width,
                            key_code,
                            key_symbol,
                            mouse_button,
                        };
                        eval_callback1_event(widget_pattern, event);
                    } else if input.starts_with("cb1f") {
                        // -- callback 1 with float
                        let parts: Vec<&str> = input.split('-').collect();
                        let widget = parts[1].trim();
                        let value = parts[2].trim().parse::<f64>().unwrap_or(0.0);
                        eval_callback1_float(widget, value);
                    } else if let Some(font) = input.strip_prefix("font") {
                        // -- callback 1 with font
                        let font = font.trim();
                        if let Ok(font) = font.parse::<font::TkFont>() {
                            eval_callback1_font("font", font);
                        }
                    } else if input.starts_with("exit") {
                        // -- wish has exited
                        kill_wish();
                        return; // exit loop and program
                    }
                }
            }
        }
    }
}

/// Creates a connection with the "wish" program.
pub fn start_wish() -> Result<toplevel::TkTopLevel, TkError> {
    start_with("wish")
}

/// Creates a connection with the given wish/tclkit program.
pub fn start_with(wish: &str) -> Result<toplevel::TkTopLevel, TkError> {
    if TRACE_WISH.set(false).is_ok() {
        start_tk_connection(wish)
    } else {
        Err(TkError {
            message: String::from("Failed to set trace option"),
        })
    }
}

/// Creates a connection with the given wish/tclkit program with
/// debugging output enabled (wish interactions are reported to stdout).
pub fn trace_with(wish: &str) -> Result<toplevel::TkTopLevel, TkError> {
    if TRACE_WISH.set(true).is_ok() {
        start_tk_connection(wish)
    } else {
        Err(TkError {
            message: String::from("Failed to set trace option"),
        })
    }
}

/// Creates a connection with the given wish/tclkit program.
fn start_tk_connection(wish: &str) -> Result<toplevel::TkTopLevel, TkError> {
    let err_msg = format!("Do not start {} twice", wish);

    unsafe {
        if let Ok(wish_process) = process::Command::new(wish)
            .stdin(process::Stdio::piped())
            .stdout(process::Stdio::piped())
            .spawn()
        {
            if WISH.set(wish_process).is_err() {
                return Err(TkError { message: err_msg });
            }
        } else {
            return Err(TkError {
                message: format!("Failed to start {} process", wish),
            });
        };

        let mut input = WISH.get_mut().unwrap().stdin.take().unwrap();
        if OUTPUT
            .set(WISH.get_mut().unwrap().stdout.take().unwrap())
            .is_err()
        {
            return Err(TkError { message: err_msg });
        }

        // -- initial setup of Tcl/Tk environment

        // set close button to output 'exit' message, so rust can close connection
        input
            .write_all(b"wm protocol . WM_DELETE_WINDOW { puts stdout {exit} ; flush stdout } \n")
            .unwrap();
        // remove the 'tearoff' menu option
        input.write_all(b"option add *tearOff 0\n").unwrap();
        // tcl function to help working with font chooser
        input
            .write_all(
                b"proc font_choice {w font args} {
            set res {font }
            append res [font actual $font]
                puts $res
                flush stdout
        }\n",
            )
            .unwrap();
        // tcl function to help working with scale widget
        input
            .write_all(
                b"proc scale_value {w value args} {
            puts cb1f-$w-$value
                flush stdout
        }\n",
            )
            .unwrap();

        // configure the communication encoding
        input
            .write_all(b"chan configure stdin -encoding utf-8\n")
            .unwrap();

        let (sender, receiver) = mpsc::channel();
        SENDER.set(sender).expect(&err_msg);

        // create thread to receive strings to send on to wish
        thread::spawn(move || loop {
            let msg: Result<String, mpsc::RecvError> = receiver.recv();
            if let Ok(msg) = msg {
                input.write_all(msg.as_bytes()).unwrap();
                input.write_all(b"\n").unwrap();
            }
        });
    }

    Ok(toplevel::TkTopLevel {
        id: String::from("."),
    })
}

/// Used to cleanly end the wish process and current rust program.
pub fn end_wish() {
    kill_wish();
    process::exit(0);
}

// Splits tcl string where items can be single words or grouped in {..}
pub(super) fn split_items(text: &str) -> Vec<String> {
    let mut result: Vec<String> = vec![];

    let mut remaining = text.trim();
    while !remaining.is_empty() {
        if let Some(start) = remaining.find('{') {
            // -- add any words before first {
            for word in remaining[..start].split_whitespace() {
                result.push(String::from(word));
            }

            if let Some(end) = remaining.find('}') {
                result.push(String::from(&remaining[start + 1..end]));
                remaining = remaining[end + 1..].trim();
            } else {
                // TODO keep what we have
                break; // panic!("Incorrectly formed font family string");
            }
        } else {
            // no { }, so just split all the words and end
            for word in remaining.split_whitespace() {
                result.push(String::from(word));
            }
            break;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_items_1() {
        let result = split_items("");
        assert_eq!(0, result.len());
    }

    #[test]
    fn split_items_2() {
        let result = split_items("abc");
        assert_eq!(1, result.len());
        assert_eq!("abc", result[0]);
    }

    #[test]
    fn split_items_3() {
        let result = split_items("  abc  def  ");
        assert_eq!(2, result.len());
        assert_eq!("abc", result[0]);
        assert_eq!("def", result[1]);
    }

    #[test]
    fn split_items_4() {
        let result = split_items("{abc def}");
        assert_eq!(1, result.len());
        assert_eq!("abc def", result[0]);
    }

    #[test]
    fn split_items_5() {
        let result = split_items("{abc def} xy_z {another}");
        assert_eq!(3, result.len());
        assert_eq!("abc def", result[0]);
        assert_eq!("xy_z", result[1]);
        assert_eq!("another", result[2]);
    }
}
