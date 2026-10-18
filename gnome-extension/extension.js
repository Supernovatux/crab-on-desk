// SPDX-License-Identifier: GPL-2.0-or-later
// crab-on-desk GNOME Shell extension: executes moves, reports positions, places the permission prompt.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const WIDGET_APP_ID = 'com.supernovatux.CrabOnDesk.Widget';
const PERMISSION_APP_ID = 'com.supernovatux.CrabOnDesk.Permission';
const OBJECT_PATH = '/com/supernovatux/CrabOnDesk';
const DISPLAY_CONFIG = 'org.gnome.Mutter.DisplayConfig';
const DISPLAY_CONFIG_PATH = '/org/gnome/Mutter/DisplayConfig';

const INTERFACE = `
<node>
  <interface name="com.supernovatux.CrabOnDesk.Shell">
    <method name="Register">
      <arg type="s" direction="out" name="connector"/>
      <arg type="(iiii)" direction="out" name="monitor"/>
    </method>
    <method name="Move">
      <arg type="i" direction="in" name="x"/>
      <arg type="i" direction="in" name="y"/>
    </method>
    <method name="Monitors">
      <arg type="a(s(iiii))" direction="out" name="monitors"/>
    </method>
    <method name="CursorPosition">
      <arg type="i" direction="out" name="x"/>
      <arg type="i" direction="out" name="y"/>
    </method>
    <method name="SetPromptSpot">
      <arg type="b" direction="in" name="left"/>
      <arg type="i" direction="in" name="edge"/>
      <arg type="i" direction="in" name="middle"/>
    </method>
    <method name="Activate">
      <arg type="u" direction="in" name="pid"/>
    </method>
    <signal name="Moved">
      <arg type="s" name="connector"/>
      <arg type="(iiii)" name="monitor"/>
      <arg type="(iiii)" name="window"/>
    </signal>
  </interface>
</node>`;

function windows() {
    return global.get_window_actors().map(actor => actor.meta_window);
}

function rectOf(rect) {
    return [rect.x, rect.y, rect.width, rect.height];
}

export default class CrabOnDesk extends Extension {
    enable() {
        this._widget = null;
        this._spot = null;
        this._connectors = [];
        this._previous = null;
        this._tracked = new Map();
        this._timeouts = new Set();
        this._exported = Gio.DBusExportedObject.wrapJSObject(INTERFACE, this);
        this._exported.export(Gio.DBus.session, OBJECT_PATH);
        this._displayHandlers = [
            global.display.connect('window-created', (_display, window) => this._watch(window)),
            global.display.connect('notify::focus-window', () => this._focusChanged()),
            global.display.connect('in-fullscreen-changed', () => this._updateLayer()),
        ];
        this._monitorsChanged = Main.layoutManager.connect('monitors-changed', () => {
            this._refreshConnectors();
            this._emitMoved();
        });
        this._refreshConnectors();
        windows().forEach(window => this._watch(window));
    }

    disable() {
        global.display.disconnect(this._displayHandlers[0]);
        global.display.disconnect(this._displayHandlers[1]);
        global.display.disconnect(this._displayHandlers[2]);
        Main.layoutManager.disconnect(this._monitorsChanged);
        for (const [window, entry] of this._tracked)
            entry.ids.forEach(id => window.disconnect(id));
        this._tracked.clear();
        this._timeouts.forEach(source => GLib.source_remove(source));
        this._timeouts.clear();
        this._forget();
        this._exported.unexport();
        this._exported = null;
        this._spot = null;
        this._previous = null;
    }

    _forget() {
        if (this._widget?.watch)
            Gio.DBus.session.unwatch_name(this._widget.watch);
        this._widget = null;
    }

    _refreshConnectors() {
        Gio.DBus.session.call(DISPLAY_CONFIG, DISPLAY_CONFIG_PATH, DISPLAY_CONFIG,
            'GetCurrentState', null, null, Gio.DBusCallFlags.NONE, -1, null,
            (connection, result) => {
                try {
                    const state = connection.call_finish(result).recursiveUnpack();
                    this._connectors = state[2].map(logical => ({
                        x: logical[0],
                        y: logical[1],
                        connector: logical[5][0][0],
                    }));
                } catch (error) {
                    console.error(`crab-on-desk: ${error}`);
                }
            });
    }

    _monitors() {
        return Main.layoutManager.monitors.map(monitor => {
            const known = this._connectors.find(c => c.x === monitor.x && c.y === monitor.y);
            return [known ? known.connector : `monitor-${monitor.index}`,
                [monitor.x, monitor.y, monitor.width, monitor.height]];
        });
    }

    _monitorOf(window) {
        const index = window ? window.get_monitor() : -1;
        const monitors = this._monitors();
        const found = monitors[index] ?? monitors[Main.layoutManager.primaryIndex];
        return found ?? ['', [0, 0, 0, 0]];
    }

    _isWidget(window) {
        return this._widget && window.get_pid() === this._widget.pid &&
            window.get_wm_class() === WIDGET_APP_ID;
    }

    _watch(window) {
        if (this._tracked.has(window)) {
            this._tracked.get(window).consider();
            return;
        }
        const entry = {ids: [], placed: false, consider: null};
        const consider = () => {
            const appId = window.get_wm_class();
            if (appId === PERMISSION_APP_ID)
                this._placePrompt(window);
            else if (this._isWidget(window))
                this._adopt(window);
        };
        entry.consider = consider;
        entry.ids.push(window.connect('notify::wm-class', consider));
        entry.ids.push(window.connect('unmanaged', () => {
            this._tracked.delete(window);
            if (this._widget?.window === window)
                this._widget.window = null;
            if (this._previous === window)
                this._previous = null;
        }));
        this._tracked.set(window, entry);
        consider();
    }

    _adopt(window) {
        if (this._widget.window === window)
            return;
        this._widget.window = window;
        this._updateLayer();
        window.stick();
        if (typeof window.hide_from_window_list === 'function')
            window.hide_from_window_list();
        this._home(window);
        const {ids} = this._tracked.get(window);
        ids.push(window.connect('shown', () => this._home(window)));

        ids.push(window.connect('position-changed', () => {
            this._keepHome(window);
            this._updateLayer();
            this._emitMoved();
        }));
    }

    _updateLayer() {
        const window = this._widget?.window;
        const actor = window?.get_compositor_private();
        if (!actor)
            return;
        if (global.display.get_monitor_in_fullscreen(window.get_monitor())) {
            window.unmake_above();
            actor.hide();
        } else {
            actor.show();
            window.make_above();
        }
    }

    _home(window, attempts = 0) {
        const frame = window.get_frame_rect();
        if (frame.width === 0) {
            if (attempts < 100) {
                const source = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 50, () => {
                    this._timeouts.delete(source);
                    this._home(window, attempts + 1);
                    return GLib.SOURCE_REMOVE;
                });
                this._timeouts.add(source);
            }
            return;
        }
        const [, [x, y, width, height]] = this._monitorOf(window);
        const home = [x + width - frame.width, y + Math.floor((height - frame.height) / 2)];
        this._widget.home = home;
        window.move_frame(true, home[0], home[1]);
    }

    _keepHome(window) {
        const home = this._widget?.home;
        if (!home || this._widget.moved)
            return;
        const frame = window.get_frame_rect();
        if (frame.x !== home[0] || frame.y !== home[1])
            window.move_frame(true, home[0], home[1]);
    }

    _emitMoved() {
        const window = this._widget?.window;
        if (!window)
            return;
        const [connector, monitor] = this._monitorOf(window);
        this._exported.emit_signal('Moved', new GLib.Variant('(s(iiii)(iiii))',
            [connector, monitor, rectOf(window.get_frame_rect())]));
    }

    _focusChanged() {
        const focused = global.display.focus_window;
        if (!focused)
            return;
        if (this._widget?.window === focused) {
            if (this._previous)
                this._previous.activate(global.get_current_time());
        } else {
            this._previous = focused;
        }
    }

    _authorised(invocation) {
        if (this._widget?.sender === invocation.get_sender())
            return true;
        invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied',
            'Only the registered widget may call this');
        return false;
    }

    RegisterAsync(_params, invocation) {
        const sender = invocation.get_sender();
        Gio.DBus.session.call('org.freedesktop.DBus', '/org/freedesktop/DBus',
            'org.freedesktop.DBus', 'GetConnectionUnixProcessID',
            new GLib.Variant('(s)', [sender]), null, Gio.DBusCallFlags.NONE, -1, null,
            (connection, result) => {
                try {
                    const [pid] = connection.call_finish(result).deepUnpack();
                    this._forget();
                    this._widget = {
                        sender,
                        pid,
                        window: null,
                        watch: Gio.DBus.session.watch_name(sender, Gio.BusNameWatcherFlags.NONE,
                            null, () => this._forget()),
                    };
                    windows().forEach(window => this._watch(window));
                    const [connector, monitor] = this._monitorOf(this._widget.window);
                    invocation.return_value(new GLib.Variant('(s(iiii))', [connector, monitor]));
                } catch (error) {
                    invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', `${error}`);
                }
            });
    }

    MoveAsync([x, y], invocation) {
        if (!this._authorised(invocation))
            return;
        const window = this._widget.window;
        this._widget.moved = true;
        if (window)
            window.move_frame(true, x, y);
        invocation.return_value(null);
    }

    MonitorsAsync(_params, invocation) {
        invocation.return_value(new GLib.Variant('(a(s(iiii)))', [this._monitors()]));
    }

    CursorPositionAsync(_params, invocation) {
        if (!this._authorised(invocation))
            return;
        const [x, y] = global.get_pointer();
        invocation.return_value(new GLib.Variant('(ii)', [x, y]));
    }

    SetPromptSpotAsync([left, edge, middle], invocation) {
        if (!this._authorised(invocation))
            return;
        this._spot = {left, edge, middle};
        invocation.return_value(null);
    }

    ActivateAsync([pid], invocation) {
        const window = windows().find(candidate => candidate.get_pid() === pid);
        if (window)
            Main.activateWindow(window);
        invocation.return_value(null);
    }

    _placePrompt(window) {
        const entry = this._tracked.get(window);
        if (!this._spot || entry.placed)
            return;
        entry.placed = true;
        const {left, edge, middle} = this._spot;
        const move = () => {
            const frame = window.get_frame_rect();
            window.move_frame(true, left ? edge - frame.width : edge, Math.round(middle - frame.height / 2));
            return frame.height;
        };
        let height = move();
        entry.ids.push(window.connect('size-changed', () => {
            const current = window.get_frame_rect().height;
            if (Math.abs(current - height) >= 1)
                height = move();
        }));
    }
}
