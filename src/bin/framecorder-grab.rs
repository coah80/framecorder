//! The panel helper: the one part of framecorder that holds a permission.
//! The recorder starts it, see framecorder::grab.

fn main() {
    let Some(card) = std::env::args_os().nth(1) else {
        eprintln!("framecorder-grab: started by the recorder, with the display to read");
        std::process::exit(2);
    };
    if let Err(e) = framecorder::grab::serve(card.as_ref()) {
        eprintln!("framecorder-grab: {e:#}");
        std::process::exit(1);
    }
}
