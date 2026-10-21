"""Bar builder: design a Waybar bar as data, generate its files, run it.

Public API used by the UI (and by whoever wires login autostart):

* ``Bar`` / ``validate_bar`` / ``load_presets`` / ``export_bar`` / ``import_bar``
* ``write_files`` — writes only into ``waybar_dir()`` (the app's own subfolder)
* ``BarProcess`` — start/stop the waybar this app launched (PID file)
* ``autostart_command()`` — the ``exec-once`` line that starts the bar on login
"""

from .generate import (files_match, generate_config, generate_menu, generate_style, paths,  # noqa: F401
                       render, strip_jsonc, waybar_dir, write_files)
from .model import (BAR_EXT, BAR_FORMAT, MAX_MODULES, POSITIONS, STYLES, WEIGHTS, ZONE_LABELS,  # noqa: F401
                    ZONES, Bar, BarError, BarPreset, data_dir, document, export_bar, import_bar,
                    load_current, load_presets, save_current, validate_bar, validate_document)
from .modules import MODULES, sample_label, workspace_labels  # noqa: F401
from .process import (INSTALL_HINTS, BarProcess, WaybarMissing, autostart_command,  # noqa: F401
                      is_dry_run, other_bars)
