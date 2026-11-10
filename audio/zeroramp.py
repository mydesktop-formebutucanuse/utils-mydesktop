#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Write a copy of an asahi-audio speaker DSP graph with PipeWire's zeroramp
# in front of each of its inputs. See audio/README.md.
#   python3 -I zeroramp.py GRAPH OUT
# GRAPH is asahi-audio's graph.json, read through spa-json-dump because it
# is SPA-JSON, not JSON. OUT is plain JSON, which PipeWire reads as well; it
# is replaced only once it has been written in full.
#
# zeroramp passes the sound through until it has seen Gap seconds of exact
# zeros, the silence PipeWire fills in where a program stops or pauses its
# sound. It then turns those zeros into a fade from the last sample down to
# zero, so that the sound does not stop mid-wave, and when the sound comes
# back it fades it in over Duration seconds. It adds no latency: it only
# rewrites zeros it has already seen in the same cycle.

import json
import os
import subprocess
import sys
import tempfile

# PipeWire's defaults: 0.666 ms, 31 samples at 48 kHz. The fade-out goes
# over the zeros counted for the gap, so Duration has to equal Gap: shorter
# leaves zeros before the fade, longer overwrites the sound before them.
# And it can only reach back within one cycle, so the gap has to fit into
# the shortest cycle PipeWire runs, 32 samples.
GAP = DURATION = 0.000666


def die(msg):
    sys.exit(f"zeroramp.py: {msg}")


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: python3 -I zeroramp.py GRAPH OUT")
    src, out = sys.argv[1:]

    try:
        dump = subprocess.run(["spa-json-dump", src], stdout=subprocess.PIPE, check=True)
        conf = json.loads(dump.stdout)
    except (OSError, subprocess.CalledProcessError, ValueError) as e:
        die(f"cannot read {src}: {e}")

    graph = conf.get("filter.graph") if isinstance(conf, dict) else None
    if not isinstance(graph, dict) or not isinstance(graph.get("nodes"), list):
        die(f"{src} has no filter.graph with nodes")
    nodes = graph["nodes"]
    names = {n.get("name") for n in nodes if isinstance(n, dict)}
    if any(isinstance(n, dict) and n.get("label") == "zeroramp" for n in nodes):
        die(f"{src} has zeroramp already")
    inputs = graph.get("inputs")
    if not isinstance(inputs, list) or not inputs:
        die(f"{src} does not list the graph's inputs")

    # The new nodes go first in the list, and filter-chain resolves a port
    # given without its node against the first node of the graph (and the
    # last). So every port has to name its node.
    def named(port):
        node, colon, _ = port.partition(":") if isinstance(port, str) else ("", "", "")
        return bool(colon) and node in names

    ports = [p for p in inputs + graph.get("outputs", []) if p not in (None, "null")]
    for link in graph.get("links", []):
        ports += [link.get("output"), link.get("input")]
    for key in ("capture.volumes", "playback.volumes"):
        ports += [v.get("control") for v in graph.get(key, [])]
    for port in ports:
        if not named(port):
            die(f"{src}: {port!r} does not name its node")

    # One zeroramp per input, named after its channel where they match up.
    positions = conf.get("capture.props", {}).get("audio.position")
    if not isinstance(positions, list) or len(positions) != len(inputs):
        positions = [str(i) for i in range(len(inputs))]
    added, links = [], []
    for i, port in enumerate(inputs):
        if port in (None, "null"):
            continue
        name = f"zeroramp_{positions[i]}"
        if name in names:
            die(f"{src} has a node called {name} already")
        added.append({
            "type": "builtin",
            "name": name,
            "label": "zeroramp",
            "control": {"Gap (s)": GAP, "Duration (s)": DURATION},
        })
        links.append({"output": f"{name}:Out", "input": port})
        inputs[i] = f"{name}:In"
    graph["nodes"] = added + nodes
    graph["links"] = links + graph.get("links", [])

    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(os.path.abspath(out)), prefix=".zeroramp-")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(conf, f, indent=4, ensure_ascii=False)
            f.write("\n")
        os.chmod(tmp, 0o644)
        os.replace(tmp, out)
    except BaseException:
        os.unlink(tmp)
        raise


if __name__ == "__main__":
    main()
