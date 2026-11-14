//! The dashboard's door. The dashboard itself is the built app in `stats/ui`,
//! served once someone's signed in; this page is all anyone else gets.

fn esc(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn login(message: &str) -> String {
    format!(
        concat!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">",
            "<meta name=\"robots\" content=\"noindex\"><title>framecorder stats</title><link rel=\"stylesheet\" href=\"/login.css\"></head><body>",
            "<main><form method=\"post\" action=\"/login\" autocomplete=\"off\">",
            "<h1>framecorder<b>.</b></h1>",
            "<label for=\"password\">Password</label><input id=\"password\" type=\"password\" name=\"password\" autocomplete=\"current-password\" maxlength=\"128\" autofocus required>",
            "<label for=\"code\">Code from your authenticator</label><input id=\"code\" type=\"text\" name=\"code\" inputmode=\"numeric\" pattern=\"[0-9]{{6}}\" maxlength=\"6\" autocomplete=\"one-time-code\" required>",
            "<p class=\"err\" role=\"alert\">{}</p><button>Open</button></form></main></body></html>"
        ),
        esc(message)
    )
}
