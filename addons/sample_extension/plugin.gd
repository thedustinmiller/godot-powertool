@tool
extends EditorPlugin
## Sample Rust Extension EditorPlugin.
##
## Defensively probes ClassDB for the classes registered by the
## .gdextension binary. If the binary is missing (e.g. user pulled a release
## built for a different platform, or hasn't run `cargo xtask build` yet),
## the plugin warns and stays inert rather than failing — matching the
## "binary-or-slow, never binary-or-bust" pattern from native-split.md.

const _NATIVE_CLASSES: Array[StringName] = [&"SampleGreeter"]


func _enter_tree() -> void:
	for cls in _NATIVE_CLASSES:
		if not ClassDB.class_exists(cls):
			push_warning(
				"sample_extension: native class %s is not registered. " % cls
				+ "The .gdextension binary is missing or incompatible for this "
				+ "platform. Run `cargo xtask build`, or download a matching "
				+ "release. Plugin staying inert."
			)
			return

	print_rich("[color=lightgreen]sample_extension: native classes registered.[/color]")


func _exit_tree() -> void:
	pass
