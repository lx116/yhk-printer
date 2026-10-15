# YHK Printer

## The idea

I had a thermal printer that only worked with its own app, so we reverse engineered it to build a TUI for everyone.

The device is a portable 57 mm thermal printer, the YHK-A32B. The goal was simple: use it as a normal printer from a computer with an open tool, rather than depend on WalkPrint, its vendor app.

## The problem

The printer did not behave like a standard receipt printer. Sending ESC/POS text did nothing. It was clearly reachable, but it did not explain how it wanted to receive a page.

A Bluetooth Low Energy scan gave us a first clue: the printer advertised a transparent-UART-style GATT service, with one write characteristic and two notification characteristics. A direct connection from a Mac was stable, although the operating system itself kept dropping the connection. The printer answered a few status questions with short replies, including hardware and firmware versions, battery voltage, print width, a serial, an error code, and a product ID. Its printable width is 384 dots, about 48 mm on 57 mm paper.

## How we found a way in

With the owner's go-ahead, we studied the WalkPrint Android package without installing or running it. Decompiling it revealed protocol facts, not reusable vendor code: the sequence for density, initialization, and bitmap data; a second grayscale bitmap format; error codes such as an open cover; and a useful surprise. What the app called authorization was only a local flag set after a server check, not a requirement enforced by the printer.

The app uses classic Bluetooth on Android, while the printer also speaks BLE. Its BLE library sends 20-byte chunks with a 20 ms pause. That helped establish a safe paced writer. Nothing from the app—code, keys, or assets—is published; only observed protocol behavior is used.

## The wall

At first, the printer accepted data and moved paper, yet produced only one thin line or nothing. We tested and discarded one explanation at a time: a wrong command format, a writer that was too fast, grayscale mode, and print density that was too low.

A ladder of six solid blocks at increasing density made six identical thin lines. That was a good failure: it ruled density out instead of merely adding another guess.

## The breakthrough

We tried three bitmap variants. Only one produced a solid bar: it sent a separate raster command for every row. The verified conclusion is that this printer prints exactly one dot row per raster command and ignores the height field. Once we sent one command per row, the first real photo printed.

The hardware taught two more practical rules. With its cover open, it silently ignores jobs and reports error code 02. And only one program can hold the Bluetooth connection at a time.

## Building it with agents

The result is a Rust CLI called `yhkp`, with a pure protocol layer, an imaging pipeline, and a paced Bluetooth writer. It has about a hundred automated tests.

An autonomous coding agent, OpenCode in auto mode, wrote the code from written specifications. Claude orchestrated and independently verified the work: rerunning tests, clippy, and formatting; rendering outputs; and inspecting them. That independent pass found defects the agent's own report missed: no tests for the security layer; a queue that never replaced the first image when a new one arrived; a typesetter that rendered only the first line of multi-line documents and crashed on horizontal rules; text 2.8 times too small because points were treated as pixels; a print-time estimate off by a factor of nine; and missing paragraph spacing.

## What it does today

The terminal UI accepts drag-and-drop files, previews images, and maintains an ordered image queue with a thumbnail grid. It offers three batch modes: print one then advance, pause between prints for tearing paper, and continuous roll.

It has natural quality levels—low, medium, and high—along with automatic, photo, text, and QR content types, plus an HD definition option. QR codes print pixel-perfect, and the printed test code scanned correctly.

Text and Markdown documents are typeset to printer width using three open-licence fonts: Atkinson Hyperlegible, Patrick Hand, and IBM Plex Mono. They printed well. There is also a local HTTP API, limited to loopback with Host and Origin checks and an optional token, so a web page cannot print behind your back. A browser editor shows the exact bitmap that will print and includes templates and a card composer.

Spacing between prints and dotted cut marks came from seeing photos print edge to edge. Cards put a photo and a message on the two faces of one piece. In “behind” mode, the printer produces the photo, asks you to cut, flip the sheet like a book page or calendar, and reinsert it; it then prints the text in a box exactly as tall as the photo. The owner tried those two steps by hand on double-sided paper, using the same bitmaps, and it worked. The guided sequence inside the TUI has not been run on the printer yet.

Sending is deliberately slow, around 380 bytes per second. A 256-row photo takes roughly 38 seconds, and a full letter takes a couple of minutes.

## What is verified and what is not

### Verified on real hardware

Connection and status queries, row-by-row printing of photos, QR and text, Markdown letter printing, and printing both faces of a card by hand (front first, then the reinserted back).

### Not verified

Continuous roll and pause mode on paper; whether print density really changes darkness (never compared side by side); how the HD option looks on paper; alignment accuracy when reinserting by hand (expect 1 to 2 mm); very long jobs; behaviour on single-sided paper (the back cannot be printed); whether the slow sending pace is actually required (it follows the vendor library and was never tested faster); and the browser editor and the TUI's guided card sequence, which have only been exercised by automated tests and API calls, not by hand in a real browser or on the printer.

## What we learned

Passing tests do not prove a hardware project works: look at the output and measure it. Test one hypothesis at a time, with a test that can fail. Keep a golden baseline before refactoring. An agent's summary is a claim to verify. And hardware has quirks nobody documents.

## What is next

The next requested feature is a web view that shows every item loaded in the TUI. The code is being refactored under SOLID and KISS principles with a byte-exact safety net. After that come packaging, a public GitHub repository, and an MIT licence.
