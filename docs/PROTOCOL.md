# Observed protocol

This document records interoperability behaviour observed on a YHK-A32B. It is not a vendor specification. Values marked **verified** were observed on real hardware; values marked **unverified** are encoded by this project but have not been confirmed on paper or hardware.

## BLE transport

| Role | Description | Status |
|---|---|---|
| Service `49535343-fe7d-4ae5-8fa9-9fafd205e455` | A transparent-UART-style BLE GATT service | Verified |
| Write characteristic `49535343-8841-43f4-a8d4-ecbe34729bb3` | Receives commands and print bytes (write and write-without-response) | Verified |
| Reply notification characteristic `49535343-1e4d-4bd9-ba61-23c647249616` | Delivers NUL-terminated query replies | Verified |
| Control characteristic `49535343-aca3-481c-91ec-d85e28a60318` | Write and notify; subscribed alongside the reply characteristic, never used to send data | Verified |

These are the generic UUIDs of a common transparent-UART BLE chip, shared by many devices; they identify the chip family, not an individual printer. The advertised device name starts with `YHK-`.

## Query commands

All replies below are NUL-terminated. Placeholder values are illustrative only.

| Command | Bytes | Reply format | Status |
|---|---|---|---|
| Status | `1e 47 03` | `HV=...,SV=...,VOLT=...mv,DPI=...,\0` | Verified |
| Serial | `1d 67 39` | `sn:XXXXXXXXXXXXXXXX.\0` | Verified |
| Error | `1d 67 53` | `err:\0.` for none, or `err:<code>.\0` | Verified |
| Product | `1d 67 69` | `public id:....\0` | Verified |
| Battery | `1b 23 4e` | Reply format unknown | Unverified |

## Print framing

Each format starts with density and initialization bytes:

| Bytes | Meaning | Status |
|---|---|---|
| `1d 49 f0 <density>` | Select density | Verified as encoded; darkness effect unverified |
| `1b 40` | Initialize the job | Verified as encoded |

Rows are packed MSB-first. A 384-dot row occupies 48 bytes.

| Format | Layout after prefix | Tail | Status |
|---|---|---|---|
| Rows | Repeated `1d 76 30 00 <width-bytes-le> 01 00 <one packed row>` | `0a 0a 0a 0a` | Verified |
| Raster | `1d 76 30 00 <width-bytes-le> <height-le> <packed rows>` | `0a 0a 0a 0a` | Unverified on paper |
| Gray | Repeated `1d 78 <width-le> <one 8-bit gray row>`, then 100 white rows | `1b 23 23 43 4c 45 4e` | Unverified on paper |

### Key finding

A `GS v 0` command with a height above one prints exactly one dot row and discards the rest on the tested printer. Rows mode therefore sends one height-one command per row and is the working default. **Verified.**

## Density

The vendor levels exposed by this project are 20 (Low), 25 (Medium), and 30 (High). The command accepts a raw byte through `--density`. The effect on printed darkness has not been compared side by side. **Levels verified as software mappings; darkness effect unverified.**

## Errors

| Code | Meaning | Status |
|---|---|---|
| `02` | Cover open; the printer ignores jobs | Verified |
| Other codes | Meaning unknown | Unverified |

## Pacing

| Fact | Status |
|---|---|
| The vendor BLE library used 20-byte writes with a 20 ms pause. | Verified from observed library behaviour |
| This project defaults to 20-byte writes, 20 ms pauses, acknowledged writes, and no additional row delay. | Verified in software |
| The resulting nominal rate is about 380 bytes per second. | Verified by calculation |
| Faster writes work reliably. | Unverified |

## Project conventions

| Convention | Behaviour | Status |
|---|---|---|
| Trailing gap | Adds blank rows after a job; defaults to 5 mm and is configurable from 0 to 30 mm. | Verified in software |
| Cut mark | When enabled with a nonzero gap, the middle gap row has edge ticks and a dotted tear guide. | Verified in software |
| Zero gap | Uses the original minimal trailer rather than adding gap rows. | Verified in software; firmware behaviour unverified |
