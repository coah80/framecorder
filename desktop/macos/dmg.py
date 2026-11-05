# dmgbuild settings for the drag-to-applications window. app.yml runs
#   dmgbuild -s macos/dmg.py -D app=... -D background=... -D icon=... framecorder out.dmg
# the background art is dmg-background.svg, rendered to the two pngs next to
# it; icon spots here have to match the empty squares in the art
import os.path

app = defines["app"]
files = [app]
symlinks = {"Applications": "/Applications"}
icon = defines["icon"]
background = defines["background"]

format = "UDZO"
window_rect = ((200, 120), (660, 400))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
icon_size = 128
text_size = 13
icon_locations = {
    os.path.basename(app): (165, 190),
    "Applications": (495, 190),
}
