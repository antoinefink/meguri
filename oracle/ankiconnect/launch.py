# AnkiConnect recorder launcher (AGPL-3.0-or-later, see ../LICENSE).
#
# Prepares a throwaway Anki base folder, then runs the pinned desktop app (aqt)
# in this process. A new base folder would make Anki show its modal language
# dialog first (ProfileManager.setDefaultLang), which nothing can answer
# offscreen, so the base is seeded through aqt's own ProfileManager: the global
# profile with language en_US, one profile named by ORACLE_PROFILE, the
# software video driver, and the add-ons of /opt/ankiconnect/addons21.
import os
import shutil
import sys
from pathlib import Path

BASE = Path(os.environ.get("ORACLE_ANKI_BASE", "/scratch/anki"))
PROFILE = os.environ.get("ORACLE_PROFILE", "oracle")
ADDONS = Path("/opt/ankiconnect/addons21")


def seed() -> None:
    from aqt.profiles import ProfileManager, VideoDriver

    BASE.mkdir(parents=True, exist_ok=True)
    pm = ProfileManager(BASE)
    pm.setupMeta()
    pm.setLang("en_US")
    pm.create(PROFILE)
    pm.set_video_driver(VideoDriver.Software)
    pm.db.close()
    dest = BASE / "addons21"
    dest.mkdir(exist_ok=True)
    for src in ADDONS.iterdir():
        shutil.copytree(src, dest / src.name, dirs_exist_ok=True)


def main() -> None:
    if not (BASE / "prefs21.db").exists():
        seed()
    import aqt

    sys.argv = ["anki", "-b", str(BASE), "-p", PROFILE]
    aqt.run()


if __name__ == "__main__":
    main()
