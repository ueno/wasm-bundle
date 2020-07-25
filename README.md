# `wasm-bundle`

`wasm-bundle` is a simple command-line tool to bundle resource files
into a WebAssembly file as a custom section.

This intends to be used in Enarx.  For the details, see the
[proposal](https://github.com/enarx/rfcs/pull/29).

## Usage

```console
$ find dir -type f | wasm-bundle input.wasm output.wasm
```
