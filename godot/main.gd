extends Node
## Builds the UI around the Rust `SpaceGame` node: main menu, HUD, chat,
## control panel and script editor. Gameplay itself lives in Rust.

const HELP_TEXT := """[b]Controls[/b]
WASD move, Space up, C down, Q/E roll (zero-g), mouse look
J jetpack on/off (off = walk on planets, Space jumps)
Z inertial dampeners, X cruise mode (raises speed cap to 5 km/s)
G build mode: 1-9, 0 or mouse wheel pick block (5 rotor, 8 gyroscope, 9 door, 0 sensor), R/T rotate, B ship/station
    LMB place (on a block, or in empty space to start a new grid), RMB remove
V first/third person (third person: mouse wheel zooms, up to 300 m when seated)
F use: sit in cockpit / open-close door / edit programmable block / control a block
K control panel of the grid you look at (or are seated in)
Enter chat & commands (/help, /tp moon, /spawn ship hover, /spawn drone, /save)
F3 stats overlay, Esc menu

[b]Scripting[/b] (Luau) - see the examples in the script editor:
  grid:block(name), grid:blocks(type?)  -> blocks of this grid and its sub-grids
  thruster:set_override(0..1), .max_thrust, .current_thrust, .forward
  piston:set_velocity(m/s), .extension, .min, .max
  hinge:set_velocity(deg/s), hinge:set_target_angle(deg | nil), .angle   (-90..90)
  rotor: same as hinge but spins forever; .angle wraps at -180..180, targets take the short way
  piston/hinge/rotor:set_free(true) switches the motor off (joint moves only from forces), .free
  gyro:set_override(vector deg/s, grid frame) steers rotation, set_override(nil) = off
  door:open(), door:close(), door:toggle(), door:set_open(bool), .is_open
  sensor.players (nearest first: .name .position .velocity .distance .seated), sensor:closest(),
    .detected, .count, .range, sensor:set_range(m)  (1-500 m)
  grid.velocity, .angular_velocity, .gravity (grid frame vectors), .altitude, .planet, .mass, .speed
  me:set_frequency(1|10|100), print(...), storage (persistent string)
  function tick(dt) ... end, function on_message(arg) ... end
"""

var game
var ui: CanvasLayer
var menu: PanelContainer
var name_edit: LineEdit
var addr_edit: LineEdit
var port_edit: LineEdit
var status_label: Label
var resume_btn: Button
var leave_btn: Button
var hud: Label
var stats_label: Label
var crosshair: Label
var chat_log: RichTextLabel
var chat_edit: LineEdit
var help_panel: PanelContainer

var panel: PanelContainer
var panel_title: Label
var panel_list: ItemList
var panel_detail: VBoxContainer
var panel_grid := -1
var panel_block := -1
var panel_ids: Array = []
var panel_value_label: Label
var panel_refresh := 0.0

var editor: PanelContainer
var editor_title: Label
var code: CodeEdit
var output: RichTextLabel
var arg_edit: LineEdit
var editor_block := -1
var editor_lines: PackedStringArray = []
var examples: Array = []

var args := {}


func _ready() -> void:
	_parse_args()
	game = ClassDB.instantiate("SpaceGame")
	add_child(game)
	ui = CanvasLayer.new()
	add_child(ui)
	_build_hud()
	_build_menu()
	_build_panel()
	_build_editor()
	_build_help()
	_update_ui_open()
	if args.has("host"):
		_on_host()
	elif args.has("join"):
		addr_edit.text = args["join"]
		_on_join()
	if args.has("autotest"):
		var t = load("res://autotest.gd").new()
		t.main = self
		add_child(t)


func _parse_args() -> void:
	var a := OS.get_cmdline_user_args()
	var i := 0
	while i < a.size():
		var k: String = a[i].trim_prefix("--")
		if i + 1 < a.size() and not a[i + 1].begins_with("--"):
			args[k] = a[i + 1]
			i += 2
		else:
			args[k] = true
			i += 1


# ------------------------------------------------------------------ building

func _panel_style(alpha := 0.88) -> StyleBoxFlat:
	var s := StyleBoxFlat.new()
	s.bg_color = Color(0.06, 0.07, 0.09, alpha)
	s.border_color = Color(0.3, 0.55, 0.8, 0.8)
	s.set_border_width_all(1)
	s.set_corner_radius_all(4)
	s.set_content_margin_all(10)
	return s


func _build_hud() -> void:
	hud = Label.new()
	hud.position = Vector2(12, 8)
	hud.add_theme_color_override("font_shadow_color", Color.BLACK)
	hud.add_theme_constant_override("shadow_offset_x", 1)
	hud.add_theme_constant_override("shadow_offset_y", 1)
	ui.add_child(hud)
	stats_label = Label.new()
	stats_label.set_anchors_preset(Control.PRESET_TOP_RIGHT)
	stats_label.position = Vector2(-560, 8)
	stats_label.size = Vector2(550, 120)
	stats_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	stats_label.add_theme_color_override("font_color", Color(0.6, 1.0, 0.6))
	stats_label.add_theme_color_override("font_shadow_color", Color.BLACK)
	ui.add_child(stats_label)
	crosshair = Label.new()
	crosshair.text = "+"
	crosshair.set_anchors_preset(Control.PRESET_CENTER)
	crosshair.position -= Vector2(4, 12)
	ui.add_child(crosshair)
	chat_log = RichTextLabel.new()
	chat_log.bbcode_enabled = true
	chat_log.scroll_following = true
	chat_log.mouse_filter = Control.MOUSE_FILTER_IGNORE
	chat_log.set_anchors_preset(Control.PRESET_BOTTOM_LEFT)
	chat_log.position = Vector2(12, -260)
	chat_log.size = Vector2(620, 210)
	chat_log.add_theme_color_override("font_shadow_color", Color.BLACK)
	ui.add_child(chat_log)
	chat_edit = LineEdit.new()
	chat_edit.set_anchors_preset(Control.PRESET_BOTTOM_LEFT)
	chat_edit.position = Vector2(12, -44)
	chat_edit.size = Vector2(620, 32)
	chat_edit.placeholder_text = "Chat or /command  (Enter to send, Esc to cancel)"
	chat_edit.visible = false
	chat_edit.text_submitted.connect(_on_chat_submitted)
	ui.add_child(chat_edit)


func _build_menu() -> void:
	menu = PanelContainer.new()
	menu.add_theme_stylebox_override("panel", _panel_style(0.95))
	menu.set_anchors_preset(Control.PRESET_CENTER)
	menu.custom_minimum_size = Vector2(420, 0)
	menu.position = Vector2(-210, -190)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 8)
	menu.add_child(v)
	var title := Label.new()
	title.text = "SPACEGAME PROTOTYPE"
	title.add_theme_font_size_override("font_size", 26)
	title.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	v.add_child(title)
	name_edit = _labeled(v, "Name", "Pilot%d" % (randi() % 1000))
	if args.has("name"):
		name_edit.text = args["name"]
	port_edit = _labeled(v, "Host port", str(args.get("port", "47800")))
	var host := Button.new()
	host.text = "Host & Play"
	host.pressed.connect(_on_host)
	v.add_child(host)
	addr_edit = _labeled(v, "Server address", "127.0.0.1:47800")
	var join := Button.new()
	join.text = "Join"
	join.pressed.connect(_on_join)
	v.add_child(join)
	resume_btn = Button.new()
	resume_btn.text = "Resume"
	resume_btn.pressed.connect(func(): _show_menu(false))
	v.add_child(resume_btn)
	leave_btn = Button.new()
	leave_btn.text = "Disconnect"
	leave_btn.pressed.connect(func(): game.leave(); _show_menu(true))
	v.add_child(leave_btn)
	var quit := Button.new()
	quit.text = "Quit"
	quit.pressed.connect(func(): get_tree().quit())
	v.add_child(quit)
	status_label = Label.new()
	status_label.autowrap_mode = TextServer.AUTOWRAP_WORD
	status_label.custom_minimum_size = Vector2(380, 40)
	v.add_child(status_label)
	ui.add_child(menu)


func _labeled(parent: Control, label: String, value: String) -> LineEdit:
	var h := HBoxContainer.new()
	var l := Label.new()
	l.text = label
	l.custom_minimum_size = Vector2(130, 0)
	h.add_child(l)
	var e := LineEdit.new()
	e.text = value
	e.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	h.add_child(e)
	parent.add_child(h)
	return e


func _build_panel() -> void:
	panel = PanelContainer.new()
	panel.add_theme_stylebox_override("panel", _panel_style())
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.custom_minimum_size = Vector2(820, 480)
	panel.position = Vector2(-410, -240)
	panel.visible = false
	var v := VBoxContainer.new()
	panel.add_child(v)
	var top := HBoxContainer.new()
	panel_title = Label.new()
	panel_title.text = "Control Panel"
	panel_title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	panel_title.add_theme_font_size_override("font_size", 20)
	top.add_child(panel_title)
	var close := Button.new()
	close.text = "Close (Esc)"
	close.pressed.connect(func(): _close_panel())
	top.add_child(close)
	v.add_child(top)
	var h := HBoxContainer.new()
	h.size_flags_vertical = Control.SIZE_EXPAND_FILL
	v.add_child(h)
	panel_list = ItemList.new()
	panel_list.custom_minimum_size = Vector2(320, 400)
	panel_list.item_selected.connect(func(i): _select_block(panel_ids[i]))
	h.add_child(panel_list)
	var scroll := ScrollContainer.new()
	scroll.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	h.add_child(scroll)
	panel_detail = VBoxContainer.new()
	panel_detail.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	panel_detail.add_theme_constant_override("separation", 8)
	scroll.add_child(panel_detail)
	ui.add_child(panel)


func _build_editor() -> void:
	editor = PanelContainer.new()
	editor.add_theme_stylebox_override("panel", _panel_style(0.96))
	editor.set_anchors_preset(Control.PRESET_FULL_RECT)
	editor.offset_left = 60
	editor.offset_top = 40
	editor.offset_right = -60
	editor.offset_bottom = -40
	editor.visible = false
	var v := VBoxContainer.new()
	editor.add_child(v)
	var top := HBoxContainer.new()
	editor_title = Label.new()
	editor_title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	editor_title.add_theme_font_size_override("font_size", 20)
	top.add_child(editor_title)
	var ex := OptionButton.new()
	ex.add_item("Load example...")
	examples = game.script_examples()
	for e in examples:
		ex.add_item(e[0])
	ex.item_selected.connect(func(i):
		if i > 0:
			code.text = examples[i - 1][1]
		ex.select(0))
	top.add_child(ex)
	var upload := Button.new()
	upload.text = "Compile & Run (Ctrl+S)"
	upload.pressed.connect(_upload_script)
	top.add_child(upload)
	var close := Button.new()
	close.text = "Close (Esc)"
	close.pressed.connect(func(): _close_editor())
	top.add_child(close)
	v.add_child(top)
	var split := VSplitContainer.new()
	split.size_flags_vertical = Control.SIZE_EXPAND_FILL
	v.add_child(split)
	code = CodeEdit.new()
	code.size_flags_vertical = Control.SIZE_EXPAND_FILL
	code.custom_minimum_size = Vector2(0, 360)
	code.gutters_draw_line_numbers = true
	code.indent_automatic = true
	code.indent_size = 2
	code.auto_brace_completion_enabled = true
	code.syntax_highlighter = _lua_highlighter()
	split.add_child(code)
	var bottom := VBoxContainer.new()
	split.add_child(bottom)
	var run := HBoxContainer.new()
	var l := Label.new()
	l.text = "Argument:"
	run.add_child(l)
	arg_edit = LineEdit.new()
	arg_edit.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	arg_edit.placeholder_text = "passed to on_message(arg)"
	arg_edit.text_submitted.connect(func(_t): _run_script())
	run.add_child(arg_edit)
	var rb := Button.new()
	rb.text = "Run"
	rb.pressed.connect(_run_script)
	run.add_child(rb)
	bottom.add_child(run)
	output = RichTextLabel.new()
	output.bbcode_enabled = true
	output.scroll_following = true
	output.custom_minimum_size = Vector2(0, 160)
	output.size_flags_vertical = Control.SIZE_EXPAND_FILL
	output.add_theme_font_override("normal_font", SystemFont.new())
	bottom.add_child(output)
	ui.add_child(editor)


func _lua_highlighter() -> CodeHighlighter:
	var h := CodeHighlighter.new()
	h.number_color = Color(0.7, 0.9, 0.6)
	h.symbol_color = Color(0.75, 0.8, 0.9)
	h.function_color = Color(0.5, 0.75, 1.0)
	h.member_variable_color = Color(0.85, 0.85, 0.7)
	for kw in ["and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while", "continue"]:
		h.add_keyword_color(kw, Color(1.0, 0.5, 0.6))
	for g in ["grid", "me", "print", "vector", "math", "string", "table", "storage", "tick", "on_message"]:
		h.add_keyword_color(g, Color(0.6, 0.85, 1.0))
	h.add_color_region("--", "", Color(0.5, 0.55, 0.6), true)
	h.add_color_region("\"", "\"", Color(0.95, 0.8, 0.5))
	h.add_color_region("'", "'", Color(0.95, 0.8, 0.5))
	return h


func _build_help() -> void:
	help_panel = PanelContainer.new()
	help_panel.add_theme_stylebox_override("panel", _panel_style(0.95))
	help_panel.set_anchors_preset(Control.PRESET_CENTER)
	help_panel.custom_minimum_size = Vector2(860, 520)
	help_panel.position = Vector2(-430, -260)
	help_panel.visible = false
	var r := RichTextLabel.new()
	r.bbcode_enabled = true
	r.text = HELP_TEXT
	help_panel.add_child(r)
	ui.add_child(help_panel)


# ------------------------------------------------------------------ actions

func _on_host() -> void:
	status_label.text = "Starting server..."
	var err: String = game.host_and_play(name_edit.text, int(port_edit.text))
	status_label.text = err if err != "" else "Connecting..."
	if err == "":
		_show_menu(false)


func _on_join() -> void:
	var err: String = game.join(name_edit.text, addr_edit.text)
	status_label.text = err if err != "" else "Connecting..."
	if err == "":
		_show_menu(false)


func _show_menu(show: bool) -> void:
	menu.visible = show
	_update_ui_open()


func _update_ui_open() -> void:
	var playing: bool = game.is_playing()
	resume_btn.visible = playing
	leave_btn.visible = playing
	var open := menu.visible or panel.visible or editor.visible or chat_edit.visible or help_panel.visible
	game.set_ui_open(open)


func _on_chat_submitted(text: String) -> void:
	game.send_chat(text)
	chat_edit.text = ""
	chat_edit.visible = false
	_update_ui_open()


func _chat(from: String, text: String) -> void:
	var color := "#8fd" if from == "server" else "#fff"
	chat_log.append_text("[color=%s]%s:[/color] %s\n" % [color, from, text.replace("[", "[lb]")])


func _input(event: InputEvent) -> void:
	if not (event is InputEventKey) or not event.pressed or event.echo:
		return
	var k: InputEventKey = event
	if k.keycode == KEY_ESCAPE:
		if chat_edit.visible:
			chat_edit.visible = false
		elif editor.visible:
			_close_editor()
		elif panel.visible:
			_close_panel()
		elif help_panel.visible:
			help_panel.visible = false
		elif game.is_playing():
			_show_menu(not menu.visible)
		_update_ui_open()
		get_viewport().set_input_as_handled()
	elif k.keycode == KEY_ENTER and game.is_playing() and not chat_edit.visible and not editor.visible and not panel.visible and not menu.visible:
		chat_edit.visible = true
		chat_edit.grab_focus()
		_update_ui_open()
		get_viewport().set_input_as_handled()
	elif k.keycode == KEY_F1 and game.is_playing():
		help_panel.visible = not help_panel.visible
		_update_ui_open()
		get_viewport().set_input_as_handled()
	elif k.keycode == KEY_F3:
		game.toggle_stats()
		get_viewport().set_input_as_handled()
	elif k.keycode == KEY_S and k.ctrl_pressed and editor.visible:
		_upload_script()
		get_viewport().set_input_as_handled()


# ------------------------------------------------------------------ panel

func open_panel(grid: int, block: int) -> void:
	panel_grid = grid
	panel.visible = true
	_refresh_panel_list()
	if block >= 0:
		_select_block(block)
	_update_ui_open()


func _close_panel() -> void:
	panel.visible = false
	panel_grid = -1
	panel_block = -1
	_update_ui_open()


func _refresh_panel_list() -> void:
	var blocks: Array = game.grid_blocks(panel_grid)
	panel_list.clear()
	panel_ids.clear()
	for b in blocks:
		var idx := panel_list.add_item("%s   (%s)" % [b["name"], b["kind_name"]])
		if not b["terminal"]:
			panel_list.set_item_custom_fg_color(idx, Color(0.6, 0.6, 0.6))
		panel_ids.append(b["id"])
	panel_title.text = "Control Panel - %d blocks" % blocks.size()
	var i := panel_ids.find(panel_block)
	if i >= 0:
		panel_list.select(i)


func _select_block(id: int) -> void:
	panel_block = id
	var i := panel_ids.find(id)
	if i >= 0:
		panel_list.select(i)
	for c in panel_detail.get_children():
		c.queue_free()
	panel_value_label = null
	var b: Dictionary = game.block_info(id)
	if b.is_empty():
		return
	var name_row := HBoxContainer.new()
	var name_e := LineEdit.new()
	name_e.text = b["name"]
	name_e.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	name_row.add_child(name_e)
	var rn := Button.new()
	rn.text = "Rename"
	rn.pressed.connect(func(): game.rename_block(id, name_e.text))
	name_e.text_submitted.connect(func(t): game.rename_block(id, t))
	name_row.add_child(rn)
	panel_detail.add_child(name_row)
	var kind_l := Label.new()
	kind_l.text = "Type: %s   (scripts use grid:block(\"%s\"))" % [b["kind_name"], b["name"]]
	panel_detail.add_child(kind_l)
	panel_value_label = Label.new()
	panel_detail.add_child(panel_value_label)
	if b["kind"] in ["piston", "hinge", "rotor"]:
		var fcb := CheckBox.new()
		fcb.text = "Free (motor off: moves only from outside forces)"
		fcb.button_pressed = b["free"]
		fcb.toggled.connect(func(on): game.set_free(id, on))
		panel_detail.add_child(fcb)
	match b["kind"]:
		"thruster":
			_slider("Override (%)", 0, 100, 1, b["override"] * 100.0, func(v): game.set_thruster_override(id, v / 100.0))
		"piston":
			_slider("Velocity (m/s)", -2, 2, 0.1, b["piston_velocity"], func(v): game.set_piston_velocity(id, v))
			_button("Reverse", func(): game.set_piston_velocity(id, -float(game.block_info(id)["piston_velocity"])); _select_block(id))
		"hinge":
			_slider("Velocity (deg/s)", -90, 90, 1, b["hinge_velocity"], func(v): game.set_hinge_velocity(id, v))
			var row := HBoxContainer.new()
			var cb := CheckBox.new()
			cb.text = "Hold target angle"
			cb.button_pressed = b["hinge_target_enabled"]
			var sp := SpinBox.new()
			sp.min_value = -90
			sp.max_value = 90
			sp.value = b["hinge_target"]
			cb.toggled.connect(func(on): game.set_hinge_target(id, on, sp.value))
			sp.value_changed.connect(func(v): if cb.button_pressed: game.set_hinge_target(id, true, v))
			row.add_child(cb)
			row.add_child(sp)
			panel_detail.add_child(row)
		"rotor":
			_slider("Velocity (deg/s)", -180, 180, 1, b["hinge_velocity"], func(v): game.set_hinge_velocity(id, v))
			var rrow := HBoxContainer.new()
			var rcb := CheckBox.new()
			rcb.text = "Hold target angle"
			rcb.button_pressed = b["hinge_target_enabled"]
			var rsp := SpinBox.new()
			rsp.min_value = -180
			rsp.max_value = 180
			rsp.value = b["hinge_target"]
			rcb.toggled.connect(func(on): game.set_hinge_target(id, on, rsp.value))
			rsp.value_changed.connect(func(v): if rcb.button_pressed: game.set_hinge_target(id, true, v))
			rrow.add_child(rcb)
			rrow.add_child(rsp)
			panel_detail.add_child(rrow)
		"programmable":
			_button("Edit script", func(): open_editor(id))
			var row := HBoxContainer.new()
			var arg := LineEdit.new()
			arg.placeholder_text = "argument"
			arg.size_flags_horizontal = Control.SIZE_EXPAND_FILL
			row.add_child(arg)
			var run := Button.new()
			run.text = "Run"
			run.pressed.connect(func(): game.run_script(id, arg.text))
			row.add_child(run)
			panel_detail.add_child(row)
		"gyro":
			var gcb := CheckBox.new()
			gcb.text = "Hold orientation (stops all rotation)"
			gcb.button_pressed = b["gyro_hold"]
			gcb.toggled.connect(func(on): game.set_gyro_hold(id, on))
			panel_detail.add_child(gcb)
			var gl := Label.new()
			gl.text = "Scripts can command turn rates with gyro:set_override(vector)."
			panel_detail.add_child(gl)
		"door":
			_button("Open / Close", func(): game.set_door(id, not game.block_info(id)["door_open"]))
		"sensor":
			_slider("Range (m)", 1, 500, 1, b["sensor_range"], func(v): game.set_sensor_range(id, v))
			var sl := Label.new()
			sl.text = "Scripts read sensor.players / sensor:closest(). The eye glows red while it sees someone."
			panel_detail.add_child(sl)
		"cockpit":
			var l := Label.new()
			l.text = "Look at the cockpit and press F to sit."
			panel_detail.add_child(l)
	_update_value_label()


func _slider(label: String, lo: float, hi: float, step: float, value: float, cb: Callable) -> void:
	var l := Label.new()
	panel_detail.add_child(l)
	var s := HSlider.new()
	s.min_value = lo
	s.max_value = hi
	s.step = step
	s.value = value
	s.custom_minimum_size = Vector2(300, 24)
	l.text = "%s: %s" % [label, str(value)]
	s.value_changed.connect(func(v):
		l.text = "%s: %s" % [label, str(v)]
		cb.call(v))
	panel_detail.add_child(s)


func _button(label: String, cb: Callable) -> void:
	var b := Button.new()
	b.text = label
	b.pressed.connect(cb)
	panel_detail.add_child(b)


func _update_value_label() -> void:
	if panel_value_label == null or not is_instance_valid(panel_value_label):
		return
	var b: Dictionary = game.block_info(panel_block)
	if b.is_empty():
		panel_value_label.text = "(block removed)"
		return
	match b["kind"]:
		"thruster":
			panel_value_label.text = "Output: %d%%" % int(b["value"] * 100.0)
		"piston":
			panel_value_label.text = "Extension: %.2f m (0 - 5)" % b["value"]
		"hinge":
			panel_value_label.text = "Angle: %.1f deg (-90 - 90)" % b["value"]
		"rotor":
			panel_value_label.text = "Angle: %.1f deg (unlimited)" % b["value"]
		"door":
			panel_value_label.text = "Open" if b["door_open"] else "Closed"
		"sensor":
			panel_value_label.text = "Detecting %d player(s) within %d m" % [int(b["value"]), int(b["sensor_range"])]
		_:
			panel_value_label.text = ""


# ------------------------------------------------------------------ editor

func open_editor(block: int) -> void:
	editor_block = block
	editor.visible = true
	panel.visible = false
	var b: Dictionary = game.block_info(block)
	editor_title.text = "Programmable Block: %s" % b.get("name", "?")
	code.text = "-- loading..."
	output.clear()
	game.request_script(block)
	code.grab_focus()
	_update_ui_open()


func _close_editor() -> void:
	editor.visible = false
	editor_block = -1
	_update_ui_open()


func _upload_script() -> void:
	if editor_block < 0:
		return
	output.clear()
	output.append_text("[color=#8af]Compiling...[/color]\n")
	game.upload_script(editor_block, code.text)


func _run_script() -> void:
	if editor_block >= 0:
		game.run_script(editor_block, arg_edit.text)


func _show_output(lines: PackedStringArray, error: String, clear: bool) -> void:
	if clear:
		output.clear()
	for l in lines:
		output.append_text(l.replace("[", "[lb]") + "\n")
	if error != "":
		output.append_text("[color=#f66]Error: %s[/color]\n" % error.replace("[", "[lb]"))


# ------------------------------------------------------------------ loop

func _process(delta: float) -> void:
	for e in game.poll_events():
		match e["type"]:
			"connected":
				_chat("server", "connected")
				_show_menu(false)
			"disconnected":
				_chat("server", "disconnected: " + e["reason"])
				status_label.text = "Disconnected: " + e["reason"]
				panel.visible = false
				editor.visible = false
				_show_menu(true)
			"chat":
				_chat(e["from"], e["text"])
			"open_panel":
				open_panel(e["grid"], e["block"])
			"open_editor":
				open_editor(e["block"])
			"script_info":
				if e["block"] == editor_block:
					code.text = e["source"] if e["source"] != "" else examples[0][1]
					_show_output(e["output"], e["error"], true)
					if e["error"] == "":
						output.append_text("[color=#8f8]OK[/color] (runs every %d tick(s))\n" % e["frequency"])
			"script_output":
				if e["block"] == editor_block:
					_show_output(e["lines"], e["error"], false)
			"grid_changed":
				if panel.visible:
					_refresh_panel_list()
			"block_control":
				pass
	hud.text = game.hud_text()
	stats_label.text = game.stats_text()
	crosshair.visible = game.is_playing() and not menu.visible
	if game.status() == "Not connected" and not menu.visible:
		_show_menu(true)
	if menu.visible and not game.is_playing():
		var st: String = game.status()
		if status_label.text == "" or status_label.text.begins_with("Connect"):
			status_label.text = st
	panel_refresh -= delta
	if panel.visible and panel_refresh <= 0.0:
		panel_refresh = 0.2
		_update_value_label()
