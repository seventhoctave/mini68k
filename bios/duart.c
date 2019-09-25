#define DUART_BASE  0x18000
#define MR1A    (* (char *) (DUART_BASE + 1))
#define MR2A    (* (char *) (DUART_BASE + 1))
#define CSRA    (* (char *) (DUART_BASE + 3))   /* Clock select reg A */

