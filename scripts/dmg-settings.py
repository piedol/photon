# dmgbuild settings for the disk image window: the app on the left, the Applications folder on
# the right, and an arrow between them on the background. scripts/make-dmg.sh passes the paths with
# -D app=... -D background=... -D icon=...
# Shared with piedol's other Mac apps and kept the same in each.
import os.path

app = defines["app"]  # noqa: F821 (dmgbuild provides `defines`)
name = os.path.basename(app)

files = [app]
symlinks = {"Applications": "/Applications"}
hide_extensions = [name]

icon = defines.get("icon")  # noqa: F821
background = defines["background"]  # noqa: F821

format = "ULFO"
filesystem = "HFS+"

window_rect = ((200, 120), (640, 400))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
show_icon_preview = False

arrange_by = None
label_pos = "bottom"
text_size = 13
icon_size = 128
icon_locations = {name: (170, 190), "Applications": (470, 190)}
