# Third-Party Notices

Godot Powertool was inspired by and is largely a port/adaptation of the following projects. The primary motivation for the port to Rust was pulling common functionality instead of potentially divergent parallel implementations; each of these are excellent in their own right.

I am grateful for their contributions to the open source ecosystem.

---

## godogen

- **Repository:** https://github.com/htdt/godogen
- **License:** MIT
- **Copyright:** Copyright (c) htdt

This was the most immediate inspiration, with the Skill noting common quirks and problems resonating deeply with my experiences. Additionally, the machinery for downloading and adapting the docs from source to markdown was adapted to Rust.

---

## godot-mcp

- **Repository:** https://github.com/Coding-Solo/godot-mcp
- **License:** MIT
- **Copyright:** Copyright (c) Coding-Solo

Most of the MCP server was ported fairly directly from this project. 

---

## godot-mcp-screenshot

- **Repository:** https://github.com/tylerhaar7/godot-mcp-screenshot
- **License:** MIT
- **Copyright:** Copyright (c) tylerhaar7

This fork of godot-mcp added a cross-platform screenshot feature that was ported and integrated with our version of godot-mcp.

---

### MIT License (applies to all three projects above)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
