/* SPDX-License-Identifier: GPL-3.0-or-later */
/*
 * scale.h: draw a dock app at Window Maker's GUI scale factor.
 *
 * A dock app draws into a pixmap of its own, 64x64 or smaller, and copies
 * it to its window. Window Maker makes its tiles 64 x GSScaleFactor pixels
 * wide but leaves the dock app's window alone, so on a HiDPI screen the dock
 * app is a small picture in the middle of a large tile. With this header
 * the dock app keeps drawing its pixmap as it always did, and makes the
 * window, its shape and the copy to the window N times as large, N being
 * the scale factor rounded down. Each pixel becomes a block of N x N
 * pixels: nothing is smoothed.
 *
 * Written for C90, as wmacpi is built with -ansi.
 */
#ifndef DOCKAPPS_SCALE_H
#define DOCKAPPS_SCALE_H

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <X11/Xlib.h>
#include <X11/extensions/Xrender.h>

/* The largest GSScaleFactor that WINGs accepts. */
#define SCALE_MAX 8

/* A factor written as "2" or "1.5"; 0 when it is not a number. */
static double scale_parse(const char *text)
{
    char *end;
    double factor = strtod(text, &end);

    return (end != text && factor > 0) ? factor : 0;
}

/* GSScaleFactor from the WMGLOBAL defaults domain, where WINGs reads it;
 * 0 when it is not set. The file is a property list:
 * { GSScaleFactor = 2; } or { "GSScaleFactor" = "1.5"; } */
static double scale_from_wmglobal(void)
{
    static const char key[] = "GSScaleFactor";
    const char *home = getenv("HOME");
    const char *root = getenv("WMAKER_USER_ROOT");
    char path[4096];
    char text[16384];
    char *p;
    size_t n;
    FILE *file;

    if (!home)
	home = "";
    if (root && root[0] == '~' && root[1] == '/')
	n = snprintf(path, sizeof(path), "%s/%s/Defaults/WMGLOBAL", home, root + 2);
    else if (root && root[0])
	n = snprintf(path, sizeof(path), "%s/Defaults/WMGLOBAL", root);
    else
	n = snprintf(path, sizeof(path), "%s/GNUstep/Defaults/WMGLOBAL", home);
    if (n >= sizeof(path))
	return 0;

    file = fopen(path, "r");
    if (!file)
	return 0;
    n = fread(text, 1, sizeof(text) - 1, file);
    fclose(file);
    text[n] = '\0';

    p = strstr(text, key);
    if (!p)
	return 0;
    p += sizeof(key) - 1;
    p += strspn(p, "\" \t\r\n");
    if (*p != '=')
	return 0;
    p++;
    p += strspn(p, "\" \t\r\n");
    return scale_parse(p);
}

/* How many times as large the dock app is to be drawn: the factor Window
 * Maker uses (GS_SCALE_FACTOR, then GSScaleFactor), rounded down so that
 * the dock app still fits in its tile. 1 when no factor is set, or when the
 * X server cannot scale (no RENDER extension). */
static int scale_factor(Display *dpy)
{
    const char *env = getenv("GS_SCALE_FACTOR");
    double factor = env ? scale_parse(env) : 0;
    int event, error;

    if (!(factor > 0))
	factor = scale_from_wmglobal();
    if (!(factor >= 2))
	return 1;
    if (!XRenderQueryExtension(dpy, &event, &error))
	return 1;
    return factor > SCALE_MAX ? SCALE_MAX : (int)factor;
}

/* Copies the top-left w x h pixels of src to dst, n times as large. Both
 * are of the given format. */
static void scale_copy(Display *dpy, Drawable src, Drawable dst,
		       XRenderPictFormat *format,
		       unsigned int w, unsigned int h, int n)
{
    /* Takes a pixel of dst to the pixel of src it shows. */
    XTransform shrink = { { { 0, 0, 0 }, { 0, 0, 0 }, { 0, 0, 0 } } };
    Picture from, to;

    shrink.matrix[0][0] = XDoubleToFixed(1.0 / n);
    shrink.matrix[1][1] = XDoubleToFixed(1.0 / n);
    shrink.matrix[2][2] = XDoubleToFixed(1.0);

    from = XRenderCreatePicture(dpy, src, format, 0, NULL);
    to = XRenderCreatePicture(dpy, dst, format, 0, NULL);
    XRenderSetPictureTransform(dpy, from, &shrink);
    XRenderSetPictureFilter(dpy, from, FilterNearest, NULL, 0);
    XRenderComposite(dpy, PictOpSrc, from, None, to, 0, 0, 0, 0, 0, 0, w * n, h * n);
    XRenderFreePicture(dpy, from);
    XRenderFreePicture(dpy, to);
}

/* Draws the top-left w x h pixels of the dock app's pixmap on its window,
 * n times as large. Both have the default visual of the screen. */
static void scale_draw(Display *dpy, Pixmap pixmap, Window win,
		       unsigned int w, unsigned int h, int n)
{
    Visual *visual = DefaultVisual(dpy, DefaultScreen(dpy));

    scale_copy(dpy, pixmap, win, XRenderFindVisualFormat(dpy, visual), w, h, n);
}

/* A shape mask (a pixmap of depth 1) of w x h pixels, n times as large. */
static Pixmap scale_mask(Display *dpy, Pixmap mask,
			 unsigned int w, unsigned int h, int n)
{
    Pixmap scaled = XCreatePixmap(dpy, mask, w * n, h * n, 1);

    scale_copy(dpy, mask, scaled, XRenderFindStandardFormat(dpy, PictStandardA1), w, h, n);
    return scaled;
}

#endif /* DOCKAPPS_SCALE_H */
