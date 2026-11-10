extends Node
## Automated smoke test, started with `-- --autotest [--shots DIR] [--expect-players N]`.
## Prints "AUTOTEST PASS" or "AUTOTEST FAIL: ..." and quits.

var main
var failures: Array = []


func _ready() -> void:
	_run()


func _wait(sec: float) -> void:
	await get_tree().create_timer(sec).timeout


func _check(ok: bool, what: String) -> void:
	print(("  ok   " if ok else "  FAIL ") + what)
	if not ok:
		failures.append(what)


func _shot(name: String) -> void:
	if not main.args.has("shots") or DisplayServer.get_name() == "headless":
		return
	await RenderingServer.frame_post_draw
	var img := get_viewport().get_texture().get_image()
	var path: String = main.args["shots"] + "/" + name + ".png"
	img.save_png(path)
	print("  shot ", path)


func _run() -> void:
	var game = main.game
	var t := 0.0
	while not game.is_playing() and t < 20.0:
		await _wait(0.25)
		t += 0.25
	_check(game.is_playing(), "connected to server (status: %s, menu: %s)" % [game.status(), main.status_label.text])
	if not game.is_playing():
		_finish()
		return
	await _wait(2.0)
	var c: Dictionary = game.debug_counts()
	_check(c["snapshots"] > 30, "receiving snapshots (%d)" % c["snapshots"])
	_check(c["grids"] >= 5, "starter grids rendered (%d)" % c["grids"])
	if main.args.has("expect-players"):
		_check(c["players"] >= int(main.args["expect-players"]), "sees other players (%d)" % c["players"])
	game.debug_set_look(0.0, -5.0)
	await _wait(0.5)
	await _shot("01_spawn")

	# Look at the starter ship and arm.
	game.debug_set_look(-15.0, -12.0)
	await _wait(0.5)
	await _shot("02_look_around")
	# Demo park: elevator, crane, robot arm, drawbridge (give them time to move).
	await _wait(4.0)
	for v in [[-60.0, 10.0, "02a_left"], [-25.0, 10.0, "02b_left_center"], [5.0, 8.0, "02c_center"], [35.0, 10.0, "02d_right"]]:
		game.debug_set_look(v[0], v[1])
		await _wait(0.6)
		await _shot(v[2])
	# Carousel at full spin (~10 s after start) and the slewing crane.
	await _wait(3.0)
	game.debug_set_look(62.0, 12.0)
	await _wait(0.5)
	await _shot("02e_carousel")
	game.debug_set_look(-35.0, 18.0)
	await _wait(0.3)
	await _shot("02f_crane")
	game.debug_look_at_block("Gyroscope", 0.0)
	await _wait(0.3)
	await _shot("02g_shuttle")
	# Cockpit views: first person, third person, zoomed out.
	game.send_chat("/sit")
	await _wait(1.0)
	await _shot("02i_cockpit_first")
	game.debug_key("V")
	await _wait(0.5)
	await _shot("02j_cockpit_third")
	game.debug_zoom(8)
	await _wait(0.3)
	await _shot("02k_cockpit_zoomed")
	game.debug_zoom(-8)
	game.debug_key("V")
	game.debug_key("F")
	await _wait(1.0)
	game.send_chat("/sit Shuttle Cockpit")
	await _wait(1.0)
	await _shot("02l_shuttle_first")
	game.debug_key("V")
	await _wait(0.5)
	await _shot("02m_shuttle_third")
	game.debug_key("V")
	game.debug_key("F")
	await _wait(1.0)
	# Back to open space above the spawn for the building checks.
	game.send_chat("/tp earth")
	await _wait(1.5)
	# Open the side door and the ramp from the flight computer, then look again.
	for gid in game.debug_grid_ids():
		for blk in game.grid_blocks(gid):
			if blk["name"] == "Shuttle Computer":
				game.run_script(blk["id"], "door")
				game.run_script(blk["id"], "ramp")
	await _wait(2.5)
	game.debug_look_at_block("Side Door Lower", 0.0)
	await _wait(0.3)
	await _shot("02h_shuttle_open")
	var found_door := false
	for gid in game.debug_grid_ids():
		for blk in game.grid_blocks(gid):
			if blk["name"] == "Side Door Lower":
				found_door = true
	_check(found_door, "shuttle with doors in the demo scene")

	# Build: new grid in front, then a block on it.
	var before: int = game.debug_counts()["grids"]
	# Face away from the demo park so the ray hits nothing.
	game.debug_set_look(180.0, -10.0)
	game.debug_key("G")
	await _wait(0.2)
	game.debug_click(false)
	await _wait(1.0)
	_check(game.debug_counts()["grids"] == before + 1, "placed a new grid")
	game.debug_click(false)
	await _wait(1.0)
	await _shot("03_building")
	game.debug_key("G")

	# Spawn a hovering ship via chat command.
	before = game.debug_counts()["grids"]
	game.send_chat("/spawn ship hover")
	await _wait(1.5)
	_check(game.debug_counts()["grids"] == before + 1, "spawned ship via /spawn")

	# A follow drone: its sensor should see us and it should come over.
	game.send_chat("/spawn drone")
	await _wait(6.0)
	var sees := false
	for gid in game.debug_grid_ids():
		for blk in game.grid_blocks(gid):
			if blk["name"] == "Drone Sensor" and game.block_info(blk["id"])["value"] >= 1.0:
				sees = true
	_check(sees, "drone sensor detects the player")
	game.debug_look_at_block("Drone Sensor", 0.0)
	await _wait(0.3)
	await _shot("04a_drone")
	game.debug_key("V")
	game.debug_set_look(150.0, -15.0)
	await _wait(0.5)
	await _shot("04b_drone_third")
	game.debug_key("V")

	# The big cruiser: spawn one and check it arrives with all its parts.
	game.send_chat("/spawn cruiser")
	await _wait(3.0)
	var frigate_parts := 0
	for gid in game.debug_grid_ids():
		var blks: Array = game.grid_blocks(gid)
		for blk in blks:
			if blk["name"] == "Cruiser Bridge":
				frigate_parts = blks.size()
	_check(frigate_parts >= 3000, "cruiser replicated with %d parts" % frigate_parts)
	game.debug_key("V")
	game.debug_zoom(20)
	game.debug_look_at_block("Cruiser Computer", 0.0)
	await _wait(1.0)
	await _shot("04c_cruiser")
	game.debug_look_at_block("Main Engine 15", 0.0)
	await _wait(0.5)
	await _shot("04d_cruiser_engines")
	print("  fps with cruiser: ", Engine.get_frames_per_second())
	game.debug_zoom(-20)
	game.debug_key("V")
	# Chase camera from the bridge: zoomed out behind and above the ship.
	game.send_chat("/sit Cruiser Bridge")
	await _wait(1.0)
	game.debug_key("V")
	game.debug_zoom(12)
	await _wait(0.8)
	await _shot("04e_cruiser_chase")
	game.debug_zoom(-12)
	game.debug_key("V")
	game.debug_key("F")
	await _wait(0.5)
	game.send_chat("/tp earth")
	await _wait(1.5)

	# Script editor round trip on the spawned ship's flight computer.
	var pb := -1
	for gid in game.debug_grid_ids():
		for b in game.grid_blocks(gid):
			if b["name"] == "Flight Computer":
				pb = b["id"]
	_check(pb >= 0, "found a programmable block")
	if pb >= 0:
		main.open_editor(pb)
		await _wait(1.0)
		_check(main.code.text.contains("Hover"), "editor loaded the hover script")
		main.code.text = "print('hello from autotest', grid.mass)\nfunction tick() end"
		main._upload_script()
		await _wait(1.0)
		_check(main.output.get_parsed_text().contains("hello from autotest"), "compiled script output shown")
		await _shot("04_editor")
		main.code.text = "function tick( broken"
		main._upload_script()
		await _wait(1.0)
		_check(main.output.get_parsed_text().contains("Error"), "compile error shown")
		main.code.text = shared_hover()
		main._upload_script()
		await _wait(0.5)
		main._close_editor()
		await _wait(6.0)
		await _shot("05_hovering")

	# Control panel on the arm.
	var arm_grid := -1
	for gid in game.debug_grid_ids():
		for b in game.grid_blocks(gid):
			if b["name"] == "Arm Piston":
				arm_grid = gid
	_check(arm_grid >= 0, "found the arm")
	if arm_grid >= 0:
		main.open_panel(arm_grid, -1)
		await _wait(0.5)
		_check(main.panel_list.item_count >= 5, "control panel lists blocks (%d)" % main.panel_list.item_count)
		await _shot("06_panel")
		main._close_panel()

	# Travel: teleport to the Moon and look at the Earth.
	game.send_chat("/tp moon")
	await _wait(2.0)
	game.debug_set_look(200.0, 25.0)
	await _wait(1.0)
	await _shot("07_moon")
	game.send_chat("/tp neptune")
	await _wait(2.0)
	game.debug_set_look(0.0, 10.0)
	await _wait(1.0)
	await _shot("08_neptune")
	game.toggle_stats()
	await _wait(0.5)
	print(game.stats_text())
	_finish()


func shared_hover() -> String:
	for e in main.examples:
		if e[0] == "Hover":
			return e[1]
	return ""


func _finish() -> void:
	if failures.is_empty():
		print("AUTOTEST PASS")
	else:
		print("AUTOTEST FAIL: ", ", ".join(failures))
	get_tree().quit(0 if failures.is_empty() else 1)
