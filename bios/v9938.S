/* V9938 video driver for mini68k v2
 *
 * Provides:
 *   - TEXT2 (80x24) console mirrored from PUTC, with scrolling
 *   - GRAPHIC4 (256x212, 16 colours) drawing through the VDP command
 *     engine: CLS, PSET, LINE, filled BOX
 *   - Sprite mode 2: 16x16 sprites, 64 patterns, 32 sprites
 *   - Palette programming
 * All access is polled; /INT is not connected on the v2 board.
 * Assembled only when V9938 is defined (see Makefile V9938=1).
 *
 * Hardware notes:
 *   Board decode places V9938 at $1C000-$1C007.
 *     $1C000  port 0  VRAM data
 *     $1C002  port 1  status read / register-write / address latch
 *     $1C004  port 2  palette
 *     $1C006  port 3  indirect register
 *   68008 short-address form used throughout: 0xC000 sign-extends to
 *   $FFFC000 and masks to $FC000 on the 20-bit bus (A16=A15=A14=1).
 *   v1 GAL decodes UARTCS from A16*A15 only (ignores A14) so enabling
 *   this driver on v1 hardware WILL corrupt the DUART. Requires v2
 *   board where UARTCS is externally qualified by A14.
 *
 *   Never use CLR/TAS/bit-ops on VDP ports: the 68000 performs a read
 *   before the write, which advances the VRAM pointer or resets the
 *   register latch.
 *
 * VRAM layout
 *   TEXT2:    0x0000 name table (80x24), 0x0800 blink table,
 *             0x1000 pattern (font) table
 *   GRAPHIC4: 0x0000-0x69FF bitmap page 0 (128 bytes/line, 212 lines),
 *             0x7400 sprite colour table, 0x7600 sprite attribute
 *             table, 0x7800 sprite pattern table (64 x 16x16)
 */

.equ VDP,       0xC000
.equ VDP_DATA,  VDP + 0
.equ VDP_CMD,   VDP + 2
.equ VDP_PAL,   VDP + 4
.equ VDP_IREG,  VDP + 6

.equ VDP_COLS,      80
.equ VDP_ROWS,      24
.equ VDP_NAME_BASE, 0x0000
.equ VDP_BLINK_BASE, 0x0800
.equ VDP_PAT_BASE,  0x1000

.equ VDP_G4_SPRCOL, 0x7400
.equ VDP_G4_SPRATR, 0x7600
.equ VDP_G4_SPRPAT, 0x7800
.equ VDP_SPR_HIDDEN, 217

/* command engine opcodes (high nibble of R46), logical op IMP = 0 */
.equ VCMD_PSET, 0x50
.equ VCMD_LINE, 0x70
.equ VCMD_LMMV, 0x80
.equ VCMD_HMMV, 0xC0

/* Cursor/mode state lives in RAM relative to %fp (SSP_INIT). The
 * offsets VDP_ROW, VDP_COL, VDP_MODE are defined in bios.S above the
 * DUART ring buffers.
 */

/* -------- VDP_WR_REG: write one register
 * in:  d0.b = register number 0..46
 *      d1.b = value
 * preserves: all
 */
VDP_WR_REG:
    moveb %d1, (VDP_CMD).w
    orib  #0x80, %d0
    moveb %d0, (VDP_CMD).w
    andib #0x3F, %d0
    rts

/* -------- VDP_WAIT: space VDP port accesses
 * The V9938 has no /WAIT output (data book p.114: pin 4 is *VDS; /WAIT is
 * a V9958 feature), so the CPU must pace itself: the data book (p.121)
 * requires 8 us between VRAM data accesses and between an address/register
 * setup and the following data byte, and 2 us between the two setup bytes.
 * Every data, palette, indirect-register and status access goes through
 * here; the bsr/rts and loop cost well over 64 clocks on the 68008 at 8 MHz.
 * preserves: all
 */
.equ VDP_WAIT_LOOPS, 1
VDP_WAIT:
    movel %d0, %sp@-
    moveq #VDP_WAIT_LOOPS, %d0
VDP_WAIT_1:
    dbf   %d0, VDP_WAIT_1
    movel %sp@+, %d0
    rts

/* -------- VDP_SET_WRITE / VDP_SET_READ: set VRAM access pointer
 * in:  d0.l = 17-bit VRAM address
 * preserves: all
 * Writes R14 (A16..A14) then the low 14 bits.
 */
VDP_SET_WRITE:
    moveml %d0-%d1, %sp@-
    moveq #0x40, %d1
    bras  VDP_SET_ADDR
VDP_SET_READ:
    moveml %d0-%d1, %sp@-
    moveq #0, %d1
VDP_SET_ADDR:
    movel %d0, %sp@-
    lsrl  #8, %d0
    lsrl  #6, %d0
    andib #0x07, %d0
    moveb %d0, (VDP_CMD).w
    moveb #0x8E, (VDP_CMD).w
    movel %sp@+, %d0
    moveb %d0, (VDP_CMD).w
    lsrw  #8, %d0
    andib #0x3F, %d0
    orb   %d1, %d0
    moveb %d0, (VDP_CMD).w
    moveml %sp@+, %d0-%d1
    rts

/* -------- VDP_FILL: write a byte value to consecutive VRAM addresses
 * in:  d0.l = VRAM address, d1.b = value, d2.w = count (>0)
 * clobbers: d2
 */
VDP_FILL:
    bsr   VDP_SET_WRITE
    subqw #1, %d2
VDP_FILL_1:
    bsr   VDP_WAIT
    moveb %d1, (VDP_DATA).w
    dbf   %d2, VDP_FILL_1
    rts

/* -------- VDP_LOAD_REGS: program registers from a table
 * in:  a0 -> (reg, value) byte pairs, terminated by reg 0xFF
 * clobbers: d0, d1, a0
 */
VDP_LOAD_REGS:
    moveb %a0@+, %d0
    bmis  VDP_LOAD_REGS_X
    moveb %a0@+, %d1
    bsr   VDP_WR_REG
    bras  VDP_LOAD_REGS
VDP_LOAD_REGS_X:
    rts

/* -------- VDP_STATUS: read a status register
 * in:  d0.b = status register number 0..9
 * out: d0.l = value (zero-extended)
 * Leaves R15 = 0. preserves: d1
 */
VDP_STATUS:
    movel %d1, %sp@-
    moveb %d0, %d1
    moveq #15, %d0
    bsr   VDP_WR_REG
    moveq #0, %d0
    bsr   VDP_WAIT
    moveb (VDP_CMD).w, %d0
    moveb #0, (VDP_CMD).w
    moveb #0x8F, (VDP_CMD).w
    movel %sp@+, %d1
    rts

/* -------- VDP_WAIT_CE: wait for the command engine to go idle
 * preserves: all
 */
VDP_WAIT_CE:
    movel %d0, %sp@-
VDP_WAIT_CE_1:
    moveq #2, %d0
    bsrs  VDP_STATUS
    btst  #0, %d0
    bnes  VDP_WAIT_CE_1
    movel %sp@+, %d0
    rts

/* -------- VDP_LOAD_PAL: load the default 16-colour palette
 * clobbers: d0, d1, a0
 */
VDP_LOAD_PAL:
    moveq #16, %d0
    moveq #0, %d1
    bsr   VDP_WR_REG
    lea   VDP_PAL_TABLE(%pc), %a0
    moveq #32 - 1, %d1
VDP_LOAD_PAL_1:
    bsr   VDP_WAIT
    moveb %a0@+, (VDP_PAL).w
    dbf   %d1, VDP_LOAD_PAL_1
    rts

/* -------- VDP_IW16: write d0.w little-endian to the indirect port
 * preserves: all
 */
VDP_IW16:
    bsr   VDP_WAIT
    moveb %d0, (VDP_IREG).w
    rorw  #8, %d0
    bsr   VDP_WAIT
    moveb %d0, (VDP_IREG).w
    rorw  #8, %d0
    rts

/* -------- VDP_CMD_EXEC: issue a command engine operation
 * in:  d1.w DX, d2.w DY, d3.w NX, d4.w NY,
 *      d5.b CLR, d6.b ARG, d7.b CMD
 * Waits for the previous command, programs R36..R46 through port 3.
 * Does not wait for this command to finish.
 * clobbers: d0
 */
VDP_CMD_EXEC:
    bsrs  VDP_WAIT_CE
    moveb #36, (VDP_CMD).w
    moveb #0x91, (VDP_CMD).w        /* R17 = 36, auto-increment */
    movew %d1, %d0
    bsrs  VDP_IW16
    movew %d2, %d0
    bsrs  VDP_IW16
    movew %d3, %d0
    bsrs  VDP_IW16
    movew %d4, %d0
    bsrs  VDP_IW16
    bsr   VDP_WAIT
    moveb %d5, (VDP_IREG).w
    bsr   VDP_WAIT
    moveb %d6, (VDP_IREG).w
    bsr   VDP_WAIT
    moveb %d7, (VDP_IREG).w
    rts

/* ==================================================================
 * Mode setup
 * ================================================================== */

/* -------- VDP_INIT: enter TEXT2 mode (power-on and 'T' command)
 * Programs registers and palette, clears the screen, loads the font,
 * homes the cursor.
 * preserves: all
 */
VDP_INIT:
    moveml %d0-%d2/%a0, %sp@-
    bsr   VDP_WAIT
    moveb (VDP_CMD).w, %d0          /* status read resets the latch */
    bsr   VDP_WAIT_CE
    lea   VDP_TEXT2_REGS(%pc), %a0
    bsr   VDP_LOAD_REGS
    bsr   VDP_LOAD_PAL
    clrb  %fp@(VDP_MODE)
    bsr   VDP_TEXT_CLEAR

    /* load font into the pattern table */
    movel #VDP_PAT_BASE, %d0
    bsr   VDP_SET_WRITE
    lea   VDP_FONT6X8(%pc), %a0
    movew #(VDP_FONT6X8_END - VDP_FONT6X8) - 1, %d2
VDP_INIT_1:
    bsr   VDP_WAIT
    moveb %a0@+, (VDP_DATA).w
    dbf   %d2, VDP_INIT_1

    moveml %sp@+, %d0-%d2/%a0
    rts

/* -------- VDP_TEXT_CLEAR: blank the TEXT2 name and blink tables and
 *          home the cursor
 * clobbers: d0, d1, d2
 */
VDP_TEXT_CLEAR:
    movel #VDP_NAME_BASE, %d0
    moveq #' ', %d1
    movew #VDP_BLINK_BASE - VDP_NAME_BASE, %d2
    bsr   VDP_FILL
    movel #VDP_BLINK_BASE, %d0
    moveq #0, %d1
    movew #272, %d2
    bsr   VDP_FILL
    clrb  %fp@(VDP_ROW)
    clrb  %fp@(VDP_COL)
    rts

/* -------- VDP_G4_INIT: enter GRAPHIC4 mode
 * Clears VRAM page 0 (bitmap + sprite tables), hides all sprites.
 * preserves: all
 */
VDP_G4_INIT:
    moveml %d0-%d7/%a0, %sp@-
    bsr   VDP_WAIT
    moveb (VDP_CMD).w, %d0
    bsr   VDP_WAIT_CE
    lea   VDP_G4_REGS(%pc), %a0
    bsr   VDP_LOAD_REGS
    bsr   VDP_LOAD_PAL
    moveb #1, %fp@(VDP_MODE)

    /* HMMV 256x256 colour 0 at (0,0): clears 0x0000-0x7FFF */
    moveq #0, %d1
    moveq #0, %d2
    movew #256, %d3
    movew #256, %d4
    moveq #0, %d5
    moveq #0, %d6
    moveb #VCMD_HMMV, %d7
    bsr   VDP_CMD_EXEC
    bsr   VDP_WAIT_CE

    /* all sprites off-screen (Y=217 hides without ending the list) */
    movel #VDP_G4_SPRATR, %d0
    bsr   VDP_SET_WRITE
    moveq #32 - 1, %d2
VDP_G4_INIT_1:
    bsr   VDP_WAIT
    moveb #VDP_SPR_HIDDEN, (VDP_DATA).w
    bsr   VDP_WAIT
    moveb #0, (VDP_DATA).w
    bsr   VDP_WAIT
    moveb #0, (VDP_DATA).w
    bsr   VDP_WAIT
    moveb #0, (VDP_DATA).w
    dbf   %d2, VDP_G4_INIT_1

    moveml %sp@+, %d0-%d7/%a0
    rts

/* ==================================================================
 * TRAP #0 API (BIOS_CALL_TABLE entries 8..16)
 * All calls preserve d1-d7/a1. d0 and a0 are scratch.
 * ================================================================== */

/* 8: VDP_SETMODE  d1.b = 0 TEXT2, otherwise GRAPHIC4 */
VDP_SETMODE:
    tstb  %d1
    beq   VDP_INIT
    bra   VDP_G4_INIT

/* 9: VDP_CLS  d1.b = colour (GRAPHIC4); TEXT2 clears and homes */
VDP_CLS:
    moveml %d1-%d7, %sp@-
    tstb  %fp@(VDP_MODE)
    bnes  VDP_CLS_G4
    bsr   VDP_TEXT_CLEAR
    bras  VDP_CLS_X
VDP_CLS_G4:
    moveb %d1, %d5
    andib #0x0F, %d5
    moveb %d5, %d0
    lslb  #4, %d0
    orb   %d0, %d5                  /* CLR = (c<<4)|c */
    moveq #0, %d1
    moveq #0, %d2
    movew #256, %d3
    movew #212, %d4
    moveq #0, %d6
    moveb #VCMD_HMMV, %d7
    bsr   VDP_CMD_EXEC
VDP_CLS_X:
    moveml %sp@+, %d1-%d7
    rts

/* 10: VDP_PSET  d1.w x, d2.w y, d3.b colour */
VDP_PSET:
    moveml %d1-%d7, %sp@-
    moveb %d3, %d5
    moveq #0, %d6
    moveb #VCMD_PSET, %d7
    bsr   VDP_CMD_EXEC
    moveml %sp@+, %d1-%d7
    rts

/* 11: VDP_LINE  d1.w x0, d2.w y0, d3.w x1, d4.w y1, d5.b colour */
VDP_LINE:
    moveml %d1-%d7, %sp@-
    moveq #0, %d6                   /* ARG */
    subw  %d1, %d3                  /* dx */
    bges  VDP_LINE_1
    negw  %d3
    bset  #2, %d6                   /* DIX: leftwards */
VDP_LINE_1:
    subw  %d2, %d4                  /* dy */
    bges  VDP_LINE_2
    negw  %d4
    bset  #3, %d6                   /* DIY: upwards */
VDP_LINE_2:
    cmpw  %d3, %d4
    bles  VDP_LINE_3
    exg   %d3, %d4                  /* long side is Y */
    bset  #0, %d6                   /* MAJ */
VDP_LINE_3:
    moveb #VCMD_LINE, %d7
    bsr   VDP_CMD_EXEC
    moveml %sp@+, %d1-%d7
    rts

/* 12: VDP_BOX  d1.w x, d2.w y, d3.w w, d4.w h, d5.b colour (filled) */
VDP_BOX:
    tstw  %d3
    beqs  VDP_BOX_X
    tstw  %d4
    beqs  VDP_BOX_X
    moveml %d1-%d7, %sp@-
    moveq #0, %d6
    moveb #VCMD_LMMV, %d7
    bsr   VDP_CMD_EXEC
    moveml %sp@+, %d1-%d7
VDP_BOX_X:
    rts

/* 13: VDP_SETPAL  d1.b index, d2.w = 0RRR0BBB 00000GGG */
VDP_SETPAL:
    movel %d1, %sp@-
    moveq #16, %d0
    andib #0x0F, %d1
    bsr   VDP_WR_REG
    movew %d2, %d0
    lsrw  #8, %d0
    bsr   VDP_WAIT
    moveb %d0, (VDP_PAL).w
    bsr   VDP_WAIT
    moveb %d2, (VDP_PAL).w
    movel %sp@+, %d1
    rts

/* 14: VDP_SPRPAT  d1.b pattern 0..63, a1 -> 32 bytes of 16x16 pattern
 * (8x8 quadrants in order: top-left, bottom-left, top-right,
 *  bottom-right) */
VDP_SPRPAT:
    movel %d1, %sp@-
    bsr   VDP_WAIT_CE
    moveq #0, %d0
    moveb %d1, %d0
    andib #63, %d0
    lslw  #5, %d0
    addl  #VDP_G4_SPRPAT, %d0
    bsr   VDP_SET_WRITE
    moveal %a1, %a0
    moveq #32 - 1, %d1
VDP_SPRPAT_1:
    bsr   VDP_WAIT
    moveb %a0@+, (VDP_DATA).w
    dbf   %d1, VDP_SPRPAT_1
    movel %sp@+, %d1
    rts

/* 15: VDP_SPRSET  d1.b sprite 0..31, d2.w x, d3.w y, d4.b pattern,
 *                 d5.b colour */
VDP_SPRSET:
    moveml %d1-%d2, %sp@-
    bsr   VDP_WAIT_CE
    andiw #31, %d1

    /* attribute entry: Y-1 (VDP draws one line lower), X, pattern*4 */
    moveq #0, %d0
    movew %d1, %d0
    lslw  #2, %d0
    addl  #VDP_G4_SPRATR, %d0
    bsr   VDP_SET_WRITE
    moveb %d3, %d0
    subqb #1, %d0
    cmpib #216, %d0                 /* 216 would end the sprite list */
    bnes  VDP_SPRSET_1
    moveb #VDP_SPR_HIDDEN, %d0
VDP_SPRSET_1:
    bsr   VDP_WAIT
    moveb %d0, (VDP_DATA).w
    bsr   VDP_WAIT
    moveb %d2, (VDP_DATA).w
    moveb %d4, %d0
    lslb  #2, %d0
    bsr   VDP_WAIT
    moveb %d0, (VDP_DATA).w
    bsr   VDP_WAIT
    moveb #0, (VDP_DATA).w

    /* colour table: same colour on all 16 lines */
    moveq #0, %d0
    movew %d1, %d0
    lslw  #4, %d0
    addl  #VDP_G4_SPRCOL, %d0
    bsr   VDP_SET_WRITE
    moveb %d5, %d0
    andib #0x0F, %d0
    moveq #16 - 1, %d2
VDP_SPRSET_2:
    bsr   VDP_WAIT
    moveb %d0, (VDP_DATA).w
    dbf   %d2, VDP_SPRSET_2

    moveml %sp@+, %d1-%d2
    rts

/* 16: VDP_SPRHIDE  d1.b sprite 0..31 */
VDP_SPRHIDE:
    movel %d1, %sp@-
    bsr   VDP_WAIT_CE
    moveq #0, %d0
    moveb %d1, %d0
    andib #31, %d0
    lslw  #2, %d0
    addl  #VDP_G4_SPRATR, %d0
    bsr   VDP_SET_WRITE
    bsr   VDP_WAIT
    moveb #VDP_SPR_HIDDEN, (VDP_DATA).w
    movel %sp@+, %d1
    rts

/* ==================================================================
 * Text console
 * ================================================================== */

/* -------- VDP_PUTC_RAW: mirror one char to the VDP console
 * in:  d2.b = character (ASCII)
 * preserves: all
 * Handles CR, LF, BS. Scrolls when the cursor moves past the last row.
 * Does nothing unless the VDP is in TEXT2 mode.
 */
VDP_PUTC_RAW:
    tstb  %fp@(VDP_MODE)
    bnes  VPR_RET
    moveml %d0-%d1/%a0, %sp@-

    cmpib #13, %d2          /* CR */
    bnes  VPR_1
    clrb  %fp@(VDP_COL)
    bras  VPR_X
VPR_1:
    cmpib #10, %d2          /* LF */
    bnes  VPR_2
    bsrs  VDP_ADVANCE_ROW
    bras  VPR_X
VPR_2:
    cmpib #8, %d2           /* BS */
    bnes  VPR_3
    tstb  %fp@(VDP_COL)
    beqs  VPR_X
    subqb #1, %fp@(VDP_COL)
    bras  VPR_X
VPR_3:
    /* printable: VRAM offset = NAME_BASE + row*80 + col */
    moveq #0, %d0
    moveb %fp@(VDP_ROW), %d0
    mulu  #VDP_COLS, %d0
    moveq #0, %d1
    moveb %fp@(VDP_COL), %d1
    addl  %d1, %d0
    addl  #VDP_NAME_BASE, %d0
    bsr   VDP_SET_WRITE
    bsr   VDP_WAIT
    moveb %d2, (VDP_DATA).w

    /* advance column, wrap to next row */
    addqb #1, %fp@(VDP_COL)
    cmpib #VDP_COLS, %fp@(VDP_COL)
    bnes  VPR_X
    clrb  %fp@(VDP_COL)
    bsrs  VDP_ADVANCE_ROW

VPR_X:
    moveml %sp@+, %d0-%d1/%a0
VPR_RET:
    rts

/* -------- VDP_ADVANCE_ROW: row++, scrolling at the bottom
 * preserves: all
 */
VDP_ADVANCE_ROW:
    addqb #1, %fp@(VDP_ROW)
    cmpib #VDP_ROWS, %fp@(VDP_ROW)
    bnes  VDP_ADVANCE_ROW_X
    subqb #1, %fp@(VDP_ROW)
    bsrs  VDP_SCROLL
VDP_ADVANCE_ROW_X:
    rts

/* -------- VDP_SCROLL: move rows 1..23 up one row, blank row 23
 * Copies each row through an 80-byte stack buffer.
 * preserves: all
 */
VDP_SCROLL:
    moveml %d0-%d3/%a0, %sp@-
    lea   %sp@(-VDP_COLS), %sp
    movel #VDP_NAME_BASE + VDP_COLS, %d3    /* source row address */
VDP_SCROLL_1:
    movel %d3, %d0
    bsr   VDP_SET_READ
    moveal %sp, %a0
    moveq #VDP_COLS - 1, %d2
VDP_SCROLL_2:
    bsr   VDP_WAIT
    moveb (VDP_DATA).w, %a0@+
    dbf   %d2, VDP_SCROLL_2

    movel %d3, %d0
    subl  #VDP_COLS, %d0
    bsr   VDP_SET_WRITE
    moveal %sp, %a0
    moveq #VDP_COLS - 1, %d2
VDP_SCROLL_3:
    bsr   VDP_WAIT
    moveb %a0@+, (VDP_DATA).w
    dbf   %d2, VDP_SCROLL_3

    addl  #VDP_COLS, %d3
    cmpl  #VDP_NAME_BASE + (VDP_COLS * VDP_ROWS), %d3
    blts  VDP_SCROLL_1

    movel #VDP_NAME_BASE + (VDP_COLS * (VDP_ROWS - 1)), %d0
    moveq #' ', %d1
    moveq #VDP_COLS, %d2
    bsr   VDP_FILL

    lea   %sp@(VDP_COLS), %sp
    moveml %sp@+, %d0-%d3/%a0
    rts

/* ==================================================================
 * 'G' monitor command: GRAPHIC4 + sprite demo
 *
 * Fixed layout (the emulator integration test probes these):
 *   colour bars: colour c fills x = c*16..c*16+15, y = 0..31
 *   frame:       colour 15 rectangle outline (0,40)-(255,211)
 *   fan:         16 lines from (128,200) to (i*16+8, 48), colour 1+i%15
 *   dots:        colour 15 at (10,50) (245,50) (10,200) (245,200)
 *   sprites:     16x16 ball, sprite 0 at (60,100) colour 15,
 *                sprite 1 at (180,100) colour 8
 * ================================================================== */
VDP_DEMO:
    moveml %d0-%d7/%a0-%a1, %sp@-
    moveq #1, %d1
    bsr   VDP_SETMODE

    /* colour bars */
    moveq #0, %d5
VDP_DEMO_1:
    movew %d5, %d1
    lslw  #4, %d1
    moveq #0, %d2
    moveq #16, %d3
    moveq #32, %d4
    bsr   VDP_BOX
    addqb #1, %d5
    cmpib #16, %d5
    bnes  VDP_DEMO_1

    /* frame */
    moveq #15, %d5
    lea   VDP_DEMO_FRAME(%pc), %a1
    moveq #4 - 1, %d7
VDP_DEMO_2:
    movew %a1@+, %d1
    movew %a1@+, %d2
    movew %a1@+, %d3
    movew %a1@+, %d4
    bsr   VDP_LINE
    dbf   %d7, VDP_DEMO_2

    /* fan */
    moveq #0, %d6
VDP_DEMO_3:
    movew #128, %d1
    movew #200, %d2
    movew %d6, %d3
    lslw  #4, %d3
    addqw #8, %d3
    moveq #48, %d4
    moveq #0, %d5
    movew %d6, %d5
    divu  #15, %d5
    swap  %d5                       /* i % 15 */
    addqb #1, %d5
    bsr   VDP_LINE
    addqw #1, %d6
    cmpiw #16, %d6
    bnes  VDP_DEMO_3

    /* corner dots */
    moveq #15, %d3
    lea   VDP_DEMO_DOTS(%pc), %a1
    moveq #4 - 1, %d7
VDP_DEMO_4:
    movew %a1@+, %d1
    movew %a1@+, %d2
    bsr   VDP_PSET
    dbf   %d7, VDP_DEMO_4

    /* sprites */
    moveq #0, %d1
    lea   VDP_DEMO_BALL(%pc), %a1
    bsr   VDP_SPRPAT
    moveq #0, %d1
    moveq #60, %d2
    moveq #100, %d3
    moveq #0, %d4
    moveq #15, %d5
    bsr   VDP_SPRSET
    moveq #1, %d1
    movew #180, %d2
    moveq #8, %d5
    bsr   VDP_SPRSET

    bsr   VDP_WAIT_CE
    moveml %sp@+, %d0-%d7/%a0-%a1
    rts

VDP_DEMO_FRAME:
    .word 0, 40, 255, 40
    .word 255, 40, 255, 211
    .word 255, 211, 0, 211
    .word 0, 211, 0, 40
VDP_DEMO_DOTS:
    .word 10, 50, 245, 50, 10, 200, 245, 200
VDP_DEMO_BALL:
    .byte 0x07, 0x1F, 0x3F, 0x7F, 0x7F, 0xFF, 0xFF, 0xFF  /* top-left */
    .byte 0xFF, 0xFF, 0xFF, 0x7F, 0x7F, 0x3F, 0x1F, 0x07  /* bottom-left */
    .byte 0xE0, 0xF8, 0xFC, 0xFE, 0xFE, 0xFF, 0xFF, 0xFF  /* top-right */
    .byte 0xFF, 0xFF, 0xFF, 0xFE, 0xFE, 0xFC, 0xF8, 0xE0  /* bottom-right */

/* ==================================================================
 * Tables
 * ================================================================== */

/* TEXT2 (80x24), NTSC, 64K VRAM chips.
 * R0  0x04  M4=1 (TEXT2 with R1 M1)
 * R1  0x50  BL=1 display on, M1=1, IE0=0
 * R2  0x03  name table 0x0000 (TEXT2 requires bits 1..0 = 1)
 * R3  0x27  blink table 0x0800 (A13..A9, low 3 bits = 1)
 * R10 0x00  blink table A16..A14
 * R4  0x02  pattern table 0x1000
 * R7  0xF4  text white on blue
 * R8  0x0A  VR=1 (64Kx4 DRAM), SPD=1 sprites off
 * R9  0x00  LN=0 192 lines, NT=0 NTSC (60 Hz)
 * R12/R13 0 blink disabled
 * R18/R23 0 no adjust / scroll
 */
VDP_TEXT2_REGS:
    .byte 0, 0x04, 1, 0x50, 2, 0x03, 3, 0x27, 10, 0x00, 4, 0x02
    .byte 5, 0x00, 6, 0x00, 11, 0x00, 7, 0xF4, 8, 0x0A, 9, 0x00
    .byte 12, 0x00, 13, 0x00, 18, 0x00, 19, 0x00, 23, 0x00
    .byte 0xFF
    .even

/* GRAPHIC4 (256x212, 16 colours), NTSC, sprite mode 2 16x16.
 * R0  0x06  M4=1 M3=1
 * R1  0x42  BL=1 display on, SI=1 16x16 sprites
 * R2  0x1F  page 0 bitmap at 0x0000
 * R5  0xEF  sprite attribute 0x7600 (colour table 0x7400), R11 = 0
 * R6  0x0F  sprite patterns 0x7800
 * R7  0x00  backdrop colour 0
 * R8  0x08  VR=1, sprites on, TP=0
 * R9  0x80  LN=1 212 lines, NTSC
 */
VDP_G4_REGS:
    .byte 0, 0x06, 1, 0x42, 2, 0x1F, 5, 0xEF, 11, 0x00, 6, 0x0F
    .byte 7, 0x00, 8, 0x08, 9, 0x80, 10, 0x00, 12, 0x00, 13, 0x00
    .byte 18, 0x00, 19, 0x00, 23, 0x00
    .byte 0xFF
    .even

/* MSX2 default palette: 0RRR0BBB, 00000GGG */
VDP_PAL_TABLE:
    .byte 0x00, 0x00, 0x00, 0x00, 0x11, 0x06, 0x33, 0x07
    .byte 0x17, 0x01, 0x27, 0x03, 0x51, 0x01, 0x27, 0x06
    .byte 0x71, 0x01, 0x73, 0x03, 0x61, 0x06, 0x64, 0x06
    .byte 0x11, 0x04, 0x65, 0x02, 0x55, 0x05, 0x77, 0x07

/* -------- 6x8 font (bios/mkfont.py -> font.bin)
 * 128 glyphs x 8 bytes, pixels in bits 7..2.
 */
VDP_FONT6X8:
    .incbin "font.bin"
VDP_FONT6X8_END:
    .even
