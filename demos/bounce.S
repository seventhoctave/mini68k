/* bounce: bouncing-ball demo for the mini68k v2 video BIOS
 *
 * Uses only the BIOS TRAP #0 API, so it runs on either VDP build
 * (V9938=1 or TMS9918=1). Draws a starry sky and a ground grid in bitmap
 * mode, then bounces three sprite balls under gravity. Any key on the
 * serial console returns to the monitor in text mode.
 *
 * Load at 0x10000 (RAM) and start it from the monitor with:  J 10000
 * In the emulator:  --load demos/bounce.s28 --type 'J 10000\r'
 */

.equ BIOS_GETC,     0
.equ BIOS_PUTS,     3
.equ VDP_SETMODE,   8
.equ VDP_CLS,       9
.equ VDP_PSET,      10
.equ VDP_LINE,      11
.equ VDP_BOX,       12
.equ VDP_SPRPAT,    14
.equ VDP_SPRSET,    15

.equ FLOOR,     150                 /* first line of the ground */
.equ BOTTOM,    211                 /* V9938 has 212 lines; the TMS9918A
                                       clips everything below line 191 */
.equ SCALE,     16                  /* positions are in 1/16 pixel */
.equ MAX_X,     (256 - 16) * SCALE
.equ MAX_Y,     (FLOOR - 16) * SCALE
.equ GRAVITY,   3                   /* 1/16 pixel per frame per frame */
.equ DELAY,     7000                /* frame pacing loop, ~60 Hz at 8 MHz */

/* ball record */
.equ B_X,       0
.equ B_Y,       2
.equ B_DX,      4
.equ B_DY,      6
.equ B_COLOR,   8
.equ B_BOUNCE,  10                  /* upward speed after hitting the floor */
.equ B_SIZE,    12
.equ BALLS,     3

    .text
    .global _start
_start:
    moveml %d2-%d7/%a2-%a5, %sp@-

    lea   MSG(%pc), %a2
    moveq #BIOS_PUTS, %d0
    trap  #0

    /* reset the ball state so the demo can be run again */
    lea   INITIAL(%pc), %a0
    lea   STATE(%pc), %a1
    moveq #(BALLS * B_SIZE / 2) - 1, %d0
COPY:
    movew %a0@+, %a1@+
    dbf   %d0, COPY

    /* scene: sky, ground, grid, stars */
    moveq #VDP_SETMODE, %d0
    moveq #1, %d1
    trap  #0
    moveq #VDP_CLS, %d0
    moveq #4, %d1                   /* dark blue sky */
    trap  #0

    moveq #VDP_BOX, %d0
    moveq #0, %d1
    movew #FLOOR, %d2
    movew #256, %d3
    moveq #BOTTOM + 1 - FLOOR, %d4
    moveq #12, %d5                  /* dark green ground */
    trap  #0

    /* grid in colour 0 (clears ground pixels on the TMS9918A, so it
     * avoids colour clash; draws black on the V9938) */
    moveq #0, %d6
GRID:
    moveq #VDP_LINE, %d0
    movew #128, %d1
    movew #FLOOR, %d2
    movew %d6, %d3
    mulu  #32, %d3
    cmpiw #255, %d3
    blss  GRID_1
    movew #255, %d3
GRID_1:
    movew #BOTTOM, %d4
    moveq #0, %d5
    trap  #0
    addqw #1, %d6
    cmpiw #9, %d6
    bnes  GRID

    lea   HLINES(%pc), %a3
    moveq #((HLINES_END - HLINES) / 2) - 1, %d7
HLINE:
    moveq #VDP_LINE, %d0
    moveq #0, %d1
    movew %a3@, %d2
    movew #255, %d3
    movew %a3@+, %d4
    moveq #0, %d5
    trap  #0
    dbf   %d7, HLINE

    lea   STARS(%pc), %a3
    moveq #((STARS_END - STARS) / 4) - 1, %d7
STAR:
    moveq #VDP_PSET, %d0
    movew %a3@+, %d1
    movew %a3@+, %d2
    moveq #15, %d3
    trap  #0
    dbf   %d7, STAR

    moveq #VDP_SPRPAT, %d0
    moveq #0, %d1
    lea   BALL(%pc), %a1
    trap  #0

FRAME:
    lea   STATE(%pc), %a5
    moveq #0, %d6                   /* ball / sprite number */
BALL_LOOP:
    /* x += dx, bounce off the side walls */
    movew %a5@(B_X), %d1
    addw  %a5@(B_DX), %d1
    bges  BX_1
    moveq #0, %d1
    negw  %a5@(B_DX)
    bras  BX_2
BX_1:
    cmpiw #MAX_X, %d1
    bles  BX_2
    movew #MAX_X, %d1
    negw  %a5@(B_DX)
BX_2:
    movew %d1, %a5@(B_X)

    /* gravity; y += dy; bounce off the floor (and the top) */
    movew %a5@(B_DY), %d2
    addqw #GRAVITY, %d2
    movew %a5@(B_Y), %d3
    addw  %d2, %d3
    cmpiw #MAX_Y, %d3
    bles  BY_1
    movew #MAX_Y, %d3
    movew %a5@(B_BOUNCE), %d2
    negw  %d2
BY_1:
    tstw  %d3
    bges  BY_2
    moveq #0, %d3
    negw  %d2
BY_2:
    movew %d2, %a5@(B_DY)
    movew %d3, %a5@(B_Y)

    /* place the sprite */
    moveq #VDP_SPRSET, %d0
    movew %d6, %d1
    movew %a5@(B_X), %d2
    asrw  #4, %d2
    asrw  #4, %d3
    moveq #0, %d4
    movew %a5@(B_COLOR), %d5
    trap  #0

    lea   %a5@(B_SIZE), %a5
    addqw #1, %d6
    cmpiw #BALLS, %d6
    bnes  BALL_LOOP

    movel #DELAY, %d0
WAIT:
    subql #1, %d0
    bnes  WAIT

    /* any key on the console ends the demo */
    moveq #BIOS_GETC, %d0
    trap  #0
    tstb  %d2
    beq   FRAME

    moveq #VDP_SETMODE, %d0
    moveq #0, %d1
    trap  #0
    moveml %sp@+, %d2-%d7/%a2-%a5
    rts

MSG:
    .asciz "Bounce demo - press any key to return\r\n"
    .even

/* x, y, dx, dy, colour, bounce speed (1/16 pixel units) */
INITIAL:
    .word 20 * SCALE, MAX_Y, 24, -80, 15, 80
    .word 100 * SCALE, 60 * SCALE, -18, 0, 8, 95
    .word 200 * SCALE, 20 * SCALE, 30, 0, 11, 110
STATE:
    .space BALLS * B_SIZE

HLINES:
    .word 156, 166, 180, 198
HLINES_END:

STARS:
    .word 12, 10, 40, 30, 70, 8, 96, 44, 130, 18, 158, 60
    .word 184, 12, 210, 38, 236, 20, 250, 70, 24, 90, 60, 120
    .word 112, 100, 170, 110, 220, 130, 140, 76
STARS_END:

BALL:
    .byte 0x07, 0x1F, 0x3F, 0x7F, 0x7F, 0xFF, 0xFF, 0xFF  /* top-left */
    .byte 0xFF, 0xFF, 0xFF, 0x7F, 0x7F, 0x3F, 0x1F, 0x07  /* bottom-left */
    .byte 0xE0, 0xF8, 0xFC, 0xFE, 0xFE, 0xFF, 0xFF, 0xFF  /* top-right */
    .byte 0xFF, 0xFF, 0xFF, 0xFE, 0xFE, 0xFC, 0xF8, 0xE0  /* bottom-right */
