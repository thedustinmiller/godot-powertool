extends Node

## ScreenshotManager — Autoload that monitors for MCP screenshot requests.
##
## Add to project.godot under [autoload]:
##   ScreenshotManager="*res://scripts/screenshot_manager.gd"
##
## The MCP server writes a request file to user://mcp_screenshot_request.txt.
## This script detects the file, captures the viewport, and saves it as
## user://mcp_screenshot.png. The MCP server polls for the output file.

const POLL_INTERVAL := 0.2  # seconds between checks
const REQUEST_FILE := "user://mcp_screenshot_request.txt"
const OUTPUT_FILE := "user://mcp_screenshot.png"

var _timer: float = 0.0


func _process(delta: float) -> void:
	_timer += delta
	if _timer < POLL_INTERVAL:
		return
	_timer = 0.0

	if not FileAccess.file_exists(REQUEST_FILE):
		return

	# Remove the request file immediately to avoid double-capture
	DirAccess.remove_absolute(ProjectSettings.globalize_path(REQUEST_FILE))

	# Wait one frame so the viewport has the latest render
	await get_tree().process_frame

	_capture_screenshot()


func _capture_screenshot() -> void:
	var viewport := get_viewport()
	if viewport == null:
		push_error("ScreenshotManager: No viewport available")
		return

	var image := viewport.get_texture().get_image()
	if image == null:
		push_error("ScreenshotManager: Failed to get viewport image")
		return

	var err := image.save_png(OUTPUT_FILE)
	if err != OK:
		push_error("ScreenshotManager: Failed to save screenshot: %s" % error_string(err))
		return

	print("ScreenshotManager: Screenshot saved to %s" % OUTPUT_FILE)
