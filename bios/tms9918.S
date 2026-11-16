/* TMS9918A video driver for mini68k v2 (alternative to v9938.S)
 *
 * Provides the same routines and TRAP #0 API as v9938.S so programs run
 * on either chip:
 *   - Text mode (40x24) console mirrored from PUTC, with scrolling
 *   - Graphics II bitmap (256x192): CLS, PSET, LINE, filled BOX done in
 *     software (the TMS9918A has no command engine). Colour is per
 *     8-pixel row segment, so neighbouring pixels share a foreground
 *     colour ("attribute clash"), as on any TMS9918A.
 *   - 32 sprites, 64 16x16 patterns (single colour per sprite)
 *   - VDP_SETPAL is accepted and ignored: the palette is fixed.
 * Assembled only when TMS9918 is defined (see Makefile TMS9918=1).
 *
 * Hardware notes:
 *   The TMS9918A goes in the v2 VDP slot instead of the V9938; only one
 *   VDP can be fitted. It decodes at the same short-address base 0xC000
 *   (A16=A15=A14=1) with MODE wired to CPU A1:
 *     $1C000  data (VRAM read/write)
 *     $1C002  control (status read / register write / address setup)
 *   16 KiB of VRAM (R1 4/16K = 1). /INT is not connected: polled driver.
 *
 *   The TMS9918A has no /WAIT output. While the display is active it
 *   needs up to 8 us between CPU VRAM accesses, so every data-port access
 *   on a live screen goes through TMS_WAIT. Bulk fills run with the
 *   display blanked, where the ~2 us minimum is met by the loop itself.
 *
 *   Never use CLR/TAS/bit-ops on VDP ports: the 68000 reads before it
 *   writes, which advances the VRAM pointer or resets the register latch.
 *
 * VRAM layout
 *   Text:        0x0000 name table (40x24), 0x0800 pattern (font) table
 *   Graphics II: 0x0000 pattern table (bitmap, 6 KiB)
 *                0x1800 sprite pattern table (64 x 16x16)
 *                0x2000 colour table (6 KiB, fg/bg per 8-pixel row)
 *                0x3800 name table (0..255 in each third)
 *                0x3B00 sprite attribute table
 */

.equ TMS,       0xC000
.equ TMS_DATA,  TMS + 0
.equ TMS_CTRL,  TMS + 2

.equ TMS_COLS,      40
.equ TMS_ROWS,      24
.equ TMS_NAME_BASE, 0x0000
.equ TMS_PAT_BASE,  0x0800

.equ G2_PAT,        0x0000
.equ G2_SPRPAT,     0x1800
.equ G2_COL,        0x2000
.equ G2_NAME,       0x3800
.equ G2_SPRATR,     0x3B00
.equ TMS_SPR_HIDDEN, 209            /* below the 192-line screen */
.equ TMS_SPR_END,   208             /* attribute-table terminator */

/* TMS_WAIT loop count. With the bsr/rts and register save the routine
 * costs well over 64 CPU clocks on the 68008 at 8 MHz (> 8 us). */
.equ TMS_WAIT_LOOPS, 1

/* R1 values: 16K VRAM, display on/off, mode bits, sprite size */
.equ TMS_R1_TEXT_ON,  0xD0
.equ TMS_R1_TEXT_OFF, 0x90
.equ TMS_R1_G2_ON,    0xC2
.equ TMS_R1_G2_OFF,   0x82

/* VDP_MODE (fp slot shared with v9938.S): 0 = text, 1 = Graphics II */

/* -------- TMS_WR_REG: write one register
 * in:  d0.b = register 0..7, d1.b = value
 * preserves: all
 */
TMS_WR_REG:
    moveb %d1, (TMS_CTRL).w
    orib  #0x80, %d0
    moveb %d0, (TMS_CTRL).w
    andib #0x07, %d0
    rts

/* -------- TMS_SET_WRITE / TMS_SET_READ: set the VRAM address
 * in:  d0.w = 14-bit address
 * preserves: all
 * TMS_SET_READ waits for the prefetch before returning.
 */
TMS_SET_WRITE:
    movew %d0, %sp@-
    moveb %d0, (TMS_CTRL).w
    lsrw  #8, %d0
    andib #0x3F, %d0
    orib  #0x40, %d0
    moveb %d0, (TMS_CTRL).w
    movew %sp@+, %d0
    rts

TMS_SET_READ:
    movew %d0, %sp@-
    moveb %d0, (TMS_CTRL).w
    lsrw  #8, %d0
    andib #0x3F, %d0
    moveb %d0, (TMS_CTRL).w
    movew %sp@+, %d0
    /* fall through to TMS_WAIT */

/* -------- TMS_WAIT: space VRAM accesses for an active display
 * preserves: all
 */
TMS_WAIT:
    movel %d0, %sp@-
    moveq #TMS_WAIT_LOOPS, %d0
TMS_WAIT_1:
    dbf   %d0, TMS_WAIT_1
    movel %sp@+, %d0
    rts

/* -------- TMS_FILL: unpaced fill, display must be blanked
 * in:  d0.w = VRAM address, d1.b = value, d2.w = count (>0)
 * clobbers: d2
 */
TMS_FILL:
    bsr   TMS_SET_WRITE
    subqw #1, %d2
TMS_FILL_1:
    moveb %d1, (TMS_DATA).w
    dbf   %d2, TMS_FILL_1
    rts

/* -------- TMS_LOAD_REGS: program registers from (reg, value) pairs
 * in:  a0 -> table terminated by 0xFF
 * clobbers: d0, d1, a0
 */
TMS_LOAD_REGS:
    moveb %a0@+, %d0
    bmis  TMS_LOAD_REGS_X
    moveb %a0@+, %d1
    bsr   TMS_WR_REG
    bras  TMS_LOAD_REGS
TMS_LOAD_REGS_X:
    rts

/* -------- TMS_DISPLAY_OFF / TMS_DISPLAY_ON: blank or show the screen
 * for the current VDP_MODE.
 * preserves: all
 */
TMS_DISPLAY_OFF:
    moveml %d0-%d1, %sp@-
    moveb #TMS_R1_TEXT_OFF, %d1
    tstb  %fp@(VDP_MODE)
    beqs  TMS_DISPLAY_SET
    moveb #TMS_R1_G2_OFF, %d1
    bras  TMS_DISPLAY_SET
TMS_DISPLAY_ON:
    moveml %d0-%d1, %sp@-
    moveb #TMS_R1_TEXT_ON, %d1
    tstb  %fp@(VDP_MODE)
    beqs  TMS_DISPLAY_SET
    moveb #TMS_R1_G2_ON, %d1
TMS_DISPLAY_SET:
    moveq #1, %d0
    bsr   TMS_WR_REG
    moveml %sp@+, %d0-%d1
    rts

/* ==================================================================
 * Mode setup
 * ================================================================== */

/* -------- VDP_INIT: enter text mode (power-on and 'T' command)
 * preserves: all
 */
VDP_INIT:
    moveml %d0-%d2/%a0, %sp@-
    moveb (TMS_CTRL).w, %d0         /* status read resets the latch */
    clrb  %fp@(VDP_MODE)
    lea   TMS_TEXT_REGS(%pc), %a0
    bsr   TMS_LOAD_REGS             /* leaves the display blanked */
    bsr   TMS_TEXT_CLEAR

    movew #TMS_PAT_BASE, %d0
    bsr   TMS_SET_WRITE
    lea   TMS_FONT6X8(%pc), %a0
    movew #(TMS_FONT6X8_END - TMS_FONT6X8) - 1, %d2
VDP_INIT_1:
    moveb %a0@+, (TMS_DATA).w
    dbf   %d2, VDP_INIT_1

    bsr   TMS_DISPLAY_ON
    moveml %sp@+, %d0-%d2/%a0
    rts

/* -------- TMS_TEXT_CLEAR: blank the name table, home the cursor
 * Display must be blanked. clobbers: d0, d1, d2
 */
TMS_TEXT_CLEAR:
    movew #TMS_NAME_BASE, %d0
    moveq #' ', %d1
    movew #TMS_COLS * TMS_ROWS, %d2
    bsr   TMS_FILL
    clrb  %fp@(VDP_ROW)
    clrb  %fp@(VDP_COL)
    rts

/* -------- TMS_G2_INIT: enter Graphics II bitmap mode
 * preserves: all
 */
TMS_G2_INIT:
    moveml %d0-%d2/%a0, %sp@-
    moveb (TMS_CTRL).w, %d0
    moveb #1, %fp@(VDP_MODE)
    lea   TMS_G2_REGS(%pc), %a0
    bsr   TMS_LOAD_REGS             /* leaves the display blanked */

    /* bitmap + sprite patterns, then colour table (fg 0, bg 0) */
    movew #G2_PAT, %d0
    moveq #0, %d1
    movew #G2_COL - G2_PAT, %d2
    bsr   TMS_FILL
    movew #G2_COL, %d0
    movew #G2_NAME - G2_COL, %d2
    bsr   TMS_FILL

    /* name table: 0..255 in each third makes a linear bitmap */
    movew #G2_NAME, %d0
    bsr   TMS_SET_WRITE
    moveq #0, %d1
    movew #768 - 1, %d2
TMS_G2_INIT_1:
    moveb %d1, (TMS_DATA).w
    addqb #1, %d1
    dbf   %d2, TMS_G2_INIT_1

    /* all sprites below the screen */
    movew #G2_SPRATR, %d0
    bsr   TMS_SET_WRITE
    moveq #32 - 1, %d2
TMS_G2_INIT_2:
    moveb #TMS_SPR_HIDDEN, (TMS_DATA).w
    moveb #0, (TMS_DATA).w
    moveb #0, (TMS_DATA).w
    moveb #0, (TMS_DATA).w
    dbf   %d2, TMS_G2_INIT_2

    bsr   TMS_DISPLAY_ON
    moveml %sp@+, %d0-%d2/%a0
    rts

/* ==================================================================
 * TRAP #0 API (BIOS_CALL_TABLE entries 8..16), same as v9938.S
 * All calls preserve d1-d7/a1. d0 and a0 are scratch.
 * ================================================================== */

/* 8: VDP_SETMODE  d1.b = 0 text, otherwise Graphics II */
VDP_SETMODE:
    tstb  %d1
    beq   VDP_INIT
    bra   TMS_G2_INIT

/* 9: VDP_CLS  d1.b = background colour (Graphics II); text clears */
VDP_CLS:
    moveml %d1-%d2, %sp@-
    bsr   TMS_DISPLAY_OFF
    tstb  %fp@(VDP_MODE)
    bnes  VDP_CLS_G2
    bsr   TMS_TEXT_CLEAR
    bras  VDP_CLS_X
VDP_CLS_G2:
    andib #0x0F, %d1
    movel %d1, %sp@-
    movew #G2_PAT, %d0
    moveq #0, %d1
    movew #G2_SPRPAT - G2_PAT, %d2
    bsr   TMS_FILL
    movel %sp@+, %d1
    movew #G2_COL, %d0
    movew #G2_NAME - G2_COL, %d2
    bsr   TMS_FILL
VDP_CLS_X:
    bsr   TMS_DISPLAY_ON
    moveml %sp@+, %d1-%d2
    rts

/* 10: VDP_PSET  d1.w x, d2.w y, d3.b colour (0 clears the pixel) */
VDP_PSET:
    moveml %d3-%d5, %sp@-
    moveb %d3, %d5
    moveq #1, %d3
    moveq #1, %d4
    bsr   VDP_BOX
    moveml %sp@+, %d3-%d5
    rts

/* TMS_PLOT: one pixel, d1.w x, d2.w y, d5.b colour. preserves: all */
TMS_PLOT:
    moveml %d3-%d4, %sp@-
    moveq #1, %d3
    moveq #1, %d4
    bsr   VDP_BOX
    moveml %sp@+, %d3-%d4
    rts

/* 11: VDP_LINE  d1.w x0, d2.w y0, d3.w x1, d4.w y1, d5.b colour
 * Bresenham in software. */
VDP_LINE:
    moveml %d0-%d7/%a2-%a4, %sp@-
    moveaw %d3, %a2                 /* x1 */
    moveaw %d4, %a3                 /* y1 */
    moveq #1, %d3                   /* sx */
    movew %a2, %d6
    subw  %d1, %d6                  /* dx = x1 - x0 */
    bges  VDP_LINE_1
    negw  %d6
    moveq #-1, %d3
VDP_LINE_1:
    moveq #1, %d4                   /* sy */
    movew %a3, %d7
    subw  %d2, %d7                  /* dy = y1 - y0 */
    bges  VDP_LINE_2
    negw  %d7
    moveq #-1, %d4
VDP_LINE_2:
    negw  %d7                       /* d7 = -|dy| */
    movew %d6, %d0
    addw  %d7, %d0                  /* err = dx - |dy| */
VDP_LINE_3:
    bsr   TMS_PLOT
    cmpw  %a2, %d1
    bnes  VDP_LINE_4
    cmpw  %a3, %d2
    beqs  VDP_LINE_X
VDP_LINE_4:
    moveaw %d0, %a4
    addaw %a4, %a4                  /* e2 = 2 * err */
    cmpaw %d7, %a4
    blts  VDP_LINE_5                /* e2 >= dy: step x */
    addw  %d7, %d0
    addw  %d3, %d1
VDP_LINE_5:
    cmpaw %d6, %a4
    bgts  VDP_LINE_3                /* e2 <= dx: step y */
    addw  %d6, %d0
    addw  %d4, %d2
    bras  VDP_LINE_3
VDP_LINE_X:
    moveml %sp@+, %d0-%d7/%a2-%a4
    rts

/* 12: VDP_BOX  d1.w x, d2.w y, d3.w w, d4.w h, d5.b colour (filled)
 * Works a byte (8-pixel segment) at a time; colour 0 clears pixels.
 * Clipped to 256x192. */
VDP_BOX:
    tstw  %d3
    beq   VDP_BOX_RET
    tstw  %d4
    beq   VDP_BOX_RET
    moveml %d0-%d7/%a1-%a2, %sp@-
    andib #0x0F, %d5
    cmpiw #255, %d1
    bhi   VDP_BOX_X
    cmpiw #191, %d2
    bhi   VDP_BOX_X
    movew %d1, %d6                  /* x1 = min(x + w - 1, 255) */
    addw  %d3, %d6
    subqw #1, %d6
    cmpiw #255, %d6
    blss  VDP_BOX_1
    movew #255, %d6
VDP_BOX_1:
    movew %d2, %d7                  /* y1 = min(y + h - 1, 191) */
    addw  %d4, %d7
    subqw #1, %d7
    cmpiw #191, %d7
    blss  VDP_BOX_2
    movew #191, %d7
VDP_BOX_2:
    moveaw %d1, %a1                 /* x0 */
VDP_BOX_ROW:
    /* a2 = row base = (y & 0xF8) << 5 | (y & 7) */
    movew %d2, %d0
    andiw #0xF8, %d0
    lslw  #5, %d0
    movew %d2, %d1
    andiw #7, %d1
    orw   %d1, %d0
    moveaw %d0, %a2
    movew %a1, %d4                  /* px = x0 */
VDP_BOX_COL:
    movew %d4, %d3                  /* last = min(px | 7, x1) */
    oriw  #7, %d3
    cmpw  %d6, %d3
    blss  VDP_BOX_3
    movew %d6, %d3
VDP_BOX_3:
    movew %d4, %d0                  /* mask = L[px & 7] & R[last & 7] */
    andiw #7, %d0
    moveb %pc@(TMS_LMASK,%d0:w), %d1
    movew %d3, %d0
    andiw #7, %d0
    andb  %pc@(TMS_RMASK,%d0:w), %d1
    movew %d4, %d0                  /* address = row base + (px & 0xF8) */
    andiw #0xF8, %d0
    addw  %a2, %d0
    bsr   TMS_PLOT_MASK
    movew %d3, %d4
    addqw #1, %d4
    cmpw  %d6, %d4
    blss  VDP_BOX_COL
    addqw #1, %d2
    cmpw  %d7, %d2
    blss  VDP_BOX_ROW
VDP_BOX_X:
    moveml %sp@+, %d0-%d7/%a1-%a2
VDP_BOX_RET:
    rts

TMS_LMASK:
    .byte 0xFF, 0x7F, 0x3F, 0x1F, 0x0F, 0x07, 0x03, 0x01
TMS_RMASK:
    .byte 0x80, 0xC0, 0xE0, 0xF0, 0xF8, 0xFC, 0xFE, 0xFF

/* -------- TMS_PLOT_MASK: set (or clear) pixels in one bitmap byte
 * in:  d0.w = pattern-table offset, d1.b = pixel mask,
 *      d5.b = colour 0..15 (0 clears the pixels, otherwise sets them and
 *             makes the colour the foreground of that 8-pixel segment)
 * preserves: all
 */
TMS_PLOT_MASK:
    moveml %d0-%d3, %sp@-
    bsr   TMS_SET_READ
    moveb (TMS_DATA).w, %d2
    tstb  %d5
    beqs  TMS_PLOT_MASK_1
    orb   %d1, %d2
    bras  TMS_PLOT_MASK_2
TMS_PLOT_MASK_1:
    notb  %d1
    andb  %d1, %d2
TMS_PLOT_MASK_2:
    bsr   TMS_SET_WRITE
    bsr   TMS_WAIT
    moveb %d2, (TMS_DATA).w
    tstb  %d5
    beqs  TMS_PLOT_MASK_X

    addiw #G2_COL, %d0
    bsr   TMS_SET_READ
    moveb (TMS_DATA).w, %d2
    andib #0x0F, %d2
    moveb %d5, %d3
    lslb  #4, %d3
    orb   %d3, %d2
    bsr   TMS_SET_WRITE
    bsr   TMS_WAIT
    moveb %d2, (TMS_DATA).w
TMS_PLOT_MASK_X:
    moveml %sp@+, %d0-%d3
    rts

/* 13: VDP_SETPAL  fixed palette on the TMS9918A: no-op */
VDP_SETPAL:
    rts

/* 14: VDP_SPRPAT  d1.b pattern 0..63, a1 -> 32 bytes (TL, BL, TR, BR) */
VDP_SPRPAT:
    movel %d1, %sp@-
    moveq #0, %d0
    moveb %d1, %d0
    andib #63, %d0
    lslw  #5, %d0
    addiw #G2_SPRPAT, %d0
    bsr   TMS_SET_WRITE
    moveal %a1, %a0
    moveq #32 - 1, %d1
VDP_SPRPAT_1:
    bsr   TMS_WAIT
    moveb %a0@+, (TMS_DATA).w
    dbf   %d1, VDP_SPRPAT_1
    movel %sp@+, %d1
    rts

/* 15: VDP_SPRSET  d1.b sprite 0..31, d2.w x, d3.w y, d4.b pattern,
 *                 d5.b colour */
VDP_SPRSET:
    movel %d1, %sp@-
    moveq #0, %d0
    moveb %d1, %d0
    andib #31, %d0
    lslw  #2, %d0
    addiw #G2_SPRATR, %d0
    bsr   TMS_SET_WRITE
    moveb %d3, %d0                  /* Y-1: the VDP draws one line lower */
    subqb #1, %d0
    cmpib #TMS_SPR_END, %d0         /* 208 would end the sprite list */
    bnes  VDP_SPRSET_1
    moveb #TMS_SPR_HIDDEN, %d0
VDP_SPRSET_1:
    bsr   TMS_WAIT
    moveb %d0, (TMS_DATA).w
    bsr   TMS_WAIT
    moveb %d2, (TMS_DATA).w
    moveb %d4, %d0
    lslb  #2, %d0
    bsr   TMS_WAIT
    moveb %d0, (TMS_DATA).w
    moveb %d5, %d0
    andib #0x0F, %d0
    bsr   TMS_WAIT
    moveb %d0, (TMS_DATA).w
    movel %sp@+, %d1
    rts

/* 16: VDP_SPRHIDE  d1.b sprite 0..31 */
VDP_SPRHIDE:
    moveq #0, %d0
    moveb %d1, %d0
    andib #31, %d0
    lslw  #2, %d0
    addiw #G2_SPRATR, %d0
    bsr   TMS_SET_WRITE
    bsr   TMS_WAIT
    moveb #TMS_SPR_HIDDEN, (TMS_DATA).w
    rts

/* ==================================================================
 * Text console
 * ================================================================== */

/* -------- VDP_PUTC_RAW: mirror one char to the text console
 * in:  d2.b = character. preserves: all
 * Handles CR, LF, BS; scrolls at the bottom. Text mode only.
 */
VDP_PUTC_RAW:
    tstb  %fp@(VDP_MODE)
    bnes  TPR_RET
    moveml %d0-%d1, %sp@-

    cmpib #13, %d2
    bnes  TPR_1
    clrb  %fp@(VDP_COL)
    bras  TPR_X
TPR_1:
    cmpib #10, %d2
    bnes  TPR_2
    bsrs  TMS_ADVANCE_ROW
    bras  TPR_X
TPR_2:
    cmpib #8, %d2
    bnes  TPR_3
    tstb  %fp@(VDP_COL)
    beqs  TPR_X
    subqb #1, %fp@(VDP_COL)
    bras  TPR_X
TPR_3:
    moveq #0, %d0
    moveb %fp@(VDP_ROW), %d0
    mulu  #TMS_COLS, %d0
    moveq #0, %d1
    moveb %fp@(VDP_COL), %d1
    addw  %d1, %d0
    addiw #TMS_NAME_BASE, %d0
    bsr   TMS_SET_WRITE
    bsr   TMS_WAIT
    moveb %d2, (TMS_DATA).w

    addqb #1, %fp@(VDP_COL)
    cmpib #TMS_COLS, %fp@(VDP_COL)
    bnes  TPR_X
    clrb  %fp@(VDP_COL)
    bsrs  TMS_ADVANCE_ROW
TPR_X:
    moveml %sp@+, %d0-%d1
TPR_RET:
    rts

/* -------- TMS_ADVANCE_ROW: row++, scrolling at the bottom
 * preserves: all
 */
TMS_ADVANCE_ROW:
    addqb #1, %fp@(VDP_ROW)
    cmpib #TMS_ROWS, %fp@(VDP_ROW)
    bnes  TMS_ADVANCE_ROW_X
    subqb #1, %fp@(VDP_ROW)
    bsrs  TMS_SCROLL
TMS_ADVANCE_ROW_X:
    rts

/* -------- TMS_SCROLL: move rows 1..23 up, blank row 23
 * Paced, so the screen stays on. preserves: all
 */
TMS_SCROLL:
    moveml %d0-%d3/%a0, %sp@-
    lea   %sp@(-TMS_COLS), %sp
    movew #TMS_NAME_BASE + TMS_COLS, %d3
TMS_SCROLL_1:
    movew %d3, %d0
    bsr   TMS_SET_READ
    moveal %sp, %a0
    moveq #TMS_COLS - 1, %d2
TMS_SCROLL_2:
    bsr   TMS_WAIT
    moveb (TMS_DATA).w, %a0@+
    dbf   %d2, TMS_SCROLL_2

    movew %d3, %d0
    subiw #TMS_COLS, %d0
    bsr   TMS_SET_WRITE
    moveal %sp, %a0
    moveq #TMS_COLS - 1, %d2
TMS_SCROLL_3:
    bsr   TMS_WAIT
    moveb %a0@+, (TMS_DATA).w
    dbf   %d2, TMS_SCROLL_3

    addiw #TMS_COLS, %d3
    cmpiw #TMS_NAME_BASE + (TMS_COLS * TMS_ROWS), %d3
    blts  TMS_SCROLL_1

    movew #TMS_NAME_BASE + (TMS_COLS * (TMS_ROWS - 1)), %d0
    bsr   TMS_SET_WRITE
    moveq #TMS_COLS - 1, %d2
TMS_SCROLL_4:
    bsr   TMS_WAIT
    moveb #' ', (TMS_DATA).w
    dbf   %d2, TMS_SCROLL_4

    lea   %sp@(TMS_COLS), %sp
    moveml %sp@+, %d0-%d3/%a0
    rts

/* ==================================================================
 * 'G' monitor command: Graphics II + sprite demo
 *
 * Fixed layout (the emulator integration test probes these):
 *   colour bars: colour c fills x = c*16..c*16+15, y = 0..31
 *   frame:       colour 15 rectangle outline (0,40)-(255,191)
 *   fan:         16 lines from (128,184) to (i*16+8, 48), colour 2+i%14
 *   dots:        colour 15 at (10,50) (245,50) (10,180) (245,180)
 *   sprites:     16x16 ball, sprite 0 at (60,100) colour 15,
 *                sprite 1 at (180,100) colour 8
 * ================================================================== */
VDP_DEMO:
    moveml %d0-%d7/%a0-%a1, %sp@-
    moveq #1, %d1
    bsr   VDP_SETMODE

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

    moveq #15, %d5
    lea   TMS_DEMO_FRAME(%pc), %a1
    moveq #4 - 1, %d7
VDP_DEMO_2:
    movew %a1@+, %d1
    movew %a1@+, %d2
    movew %a1@+, %d3
    movew %a1@+, %d4
    bsr   VDP_LINE
    dbf   %d7, VDP_DEMO_2

    moveq #0, %d6
VDP_DEMO_3:
    movew #128, %d1
    movew #184, %d2
    movew %d6, %d3
    lslw  #4, %d3
    addqw #8, %d3
    moveq #48, %d4
    moveq #0, %d5
    movew %d6, %d5
    divu  #14, %d5
    swap  %d5                       /* i % 14 */
    addqb #2, %d5
    bsr   VDP_LINE
    addqw #1, %d6
    cmpiw #16, %d6
    bnes  VDP_DEMO_3

    moveq #15, %d3
    lea   TMS_DEMO_DOTS(%pc), %a1
    moveq #4 - 1, %d7
VDP_DEMO_4:
    movew %a1@+, %d1
    movew %a1@+, %d2
    bsr   VDP_PSET
    dbf   %d7, VDP_DEMO_4

    moveq #0, %d1
    lea   TMS_DEMO_BALL(%pc), %a1
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

    moveml %sp@+, %d0-%d7/%a0-%a1
    rts

TMS_DEMO_FRAME:
    .word 0, 40, 255, 40
    .word 255, 40, 255, 191
    .word 255, 191, 0, 191
    .word 0, 191, 0, 40
TMS_DEMO_DOTS:
    .word 10, 50, 245, 50, 10, 180, 245, 180
TMS_DEMO_BALL:
    .byte 0x07, 0x1F, 0x3F, 0x7F, 0x7F, 0xFF, 0xFF, 0xFF
    .byte 0xFF, 0xFF, 0xFF, 0x7F, 0x7F, 0x3F, 0x1F, 0x07
    .byte 0xE0, 0xF8, 0xFC, 0xFE, 0xFE, 0xFF, 0xFF, 0xFF
    .byte 0xFF, 0xFF, 0xFF, 0xFE, 0xFE, 0xFC, 0xF8, 0xE0

/* ==================================================================
 * Tables
 * ================================================================== */

/* Text mode, written with the display blanked (R1 first).
 * R1 0x90  16K, display off, M1 (text)  -> TMS_DISPLAY_ON sets 0xD0
 * R0 0x00  no Graphics II, no external video
 * R2 0x00  name table 0x0000
 * R4 0x01  pattern table 0x0800
 * R7 0xF4  white text on dark blue
 */
TMS_TEXT_REGS:
    .byte 1, TMS_R1_TEXT_OFF, 0, 0x00, 2, 0x00, 3, 0x00, 4, 0x01
    .byte 5, 0x00, 6, 0x00, 7, 0xF4
    .byte 0xFF
    .even

/* Graphics II, written with the display blanked.
 * R1 0x82  16K, display off, 16x16 sprites -> TMS_DISPLAY_ON sets 0xC2
 * R0 0x02  M3 (Graphics II)
 * R2 0x0E  name table 0x3800
 * R3 0xFF  colour table 0x2000, all three thirds
 * R4 0x03  pattern table 0x0000, all three thirds
 * R5 0x76  sprite attributes 0x3B00
 * R6 0x03  sprite patterns 0x1800
 * R7 0x01  backdrop black
 */
TMS_G2_REGS:
    .byte 1, TMS_R1_G2_OFF, 0, 0x02, 2, 0x0E, 3, 0xFF, 4, 0x03
    .byte 5, 0x76, 6, 0x03, 7, 0x01
    .byte 0xFF
    .even

/* 6x8 font (bios/mkfont.py -> font.bin), pixels in bits 7..2 */
TMS_FONT6X8:
    .incbin "font.bin"
TMS_FONT6X8_END:
    .even
