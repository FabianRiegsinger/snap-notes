# dmgbuild settings for the drag-to-Applications window. bundle.sh passes
# the app and the background with -D. Icon spots match dmg-background.py.
import os.path

app = defines["app"]  # noqa: F821 (dmgbuild provides `defines`)
name = os.path.basename(app)

format = "UDZO"
files = [app]
symlinks = {"Applications": "/Applications"}
hide_extensions = [name]

background = defines["background"]  # noqa: F821
window_rect = ((200, 140), (660, 420))
default_view = "icon-view"
show_toolbar = False
show_status_bar = False
show_pathbar = False
show_sidebar = False
show_tab_view = False

icon_size = 112
text_size = 13
icon_locations = {name: (180, 235), "Applications": (480, 235)}
