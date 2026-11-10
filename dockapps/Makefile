# SPDX-License-Identifier: GPL-3.0-or-later
# Builds the dock apps into build/: wmclock, wmacpi and wmacpi-cli.
#   make
#   make install [DESTDIR=...] [PREFIX=/usr]
#   make WMCLOCK_LANG=french      day and month names of wmclock/lang.french

CC ?= cc
CFLAGS ?= -O2
PKG_CONFIG ?= pkg-config
PREFIX ?= /usr/local
WMCLOCK_LANG ?= english

WMCLOCK_VER := 1.0.16
WMCLOCK_DATE := 2015-09-24
WMACPI_VER := 2.4

X_CFLAGS := $(shell $(PKG_CONFIG) --cflags x11 xext xpm xrender)
X_LIBS := $(shell $(PKG_CONFIG) --libs x11 xext xpm xrender)
DOCKAPP_CFLAGS := $(shell $(PKG_CONFIG) --cflags dockapp)
DOCKAPP_LIBS := $(shell $(PKG_CONFIG) --libs dockapp)

WMCLOCK_DEFS := -DVERSION='"$(WMCLOCK_VER)"' \
	-DMONTH_XPM='"lang.$(WMCLOCK_LANG)/month.xpm"' \
	-DWEEKDAY_XPM='"lang.$(WMCLOCK_LANG)/weekday.xpm"'
# -ansi, -Wall and -W are the flags of wmacpi's own Makefile
WMACPI_FLAGS := -Wall -W -ansi -DWMACPI_VER='"$(WMACPI_VER)"'

bins := build/wmclock build/wmacpi build/wmacpi-cli
mans := build/wmclock.1 wmacpi/wmacpi.1 wmacpi/wmacpi-cli.1

all: $(bins) build/wmclock.1

build:
	mkdir -p build

build/wmclock: wmclock/wmclock.c wmclock/dynlist.c wmclock/dynlist.h scale.h \
		$(wildcard wmclock/xpm/* wmclock/lang.$(WMCLOCK_LANG)/*) | build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(X_CFLAGS) $(WMCLOCK_DEFS) $(LDFLAGS) -o $@ \
		wmclock/wmclock.c wmclock/dynlist.c $(X_LIBS)

build/wmclock.1: wmclock/wmclock.1.in | build
	sed -e 's|@DATE@|$(WMCLOCK_DATE)|' -e 's|@VERSION@|$(WMCLOCK_VER)|' \
		-e 's|@prefix@|$(PREFIX)|g' $< >$@

build/wmacpi: wmacpi/wmacpi.c wmacpi/libacpi.c wmacpi/libacpi.h scale.h \
		wmacpi/master.xpm wmacpi/master_low.xpm | build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WMACPI_FLAGS) $(X_CFLAGS) $(DOCKAPP_CFLAGS) \
		$(LDFLAGS) -o $@ wmacpi/wmacpi.c wmacpi/libacpi.c \
		$(DOCKAPP_LIBS) $(X_LIBS)

build/wmacpi-cli: wmacpi/wmacpi-cli.c wmacpi/libacpi.c wmacpi/libacpi.h | build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WMACPI_FLAGS) $(LDFLAGS) -o $@ \
		wmacpi/wmacpi-cli.c wmacpi/libacpi.c

install: all
	install -d $(DESTDIR)$(PREFIX)/bin $(DESTDIR)$(PREFIX)/share/man/man1
	install -m 755 $(bins) $(DESTDIR)$(PREFIX)/bin/
	install -m 644 $(mans) $(DESTDIR)$(PREFIX)/share/man/man1/
	for lang in wmclock/lang.*; do \
		install -d $(DESTDIR)$(PREFIX)/share/$$lang && \
		install -m 644 $$lang/*.xpm $(DESTDIR)$(PREFIX)/share/$$lang/ || exit 1; \
	done

clean:
	rm -rf build

.PHONY: all install clean
