/* Extract from ifduyue/musl src/ctype/isprint.c at
 * commit 7813e9119eb5fe681b6750b8a033a0490bca0941
 * upstream blob b950816bcb52dc5a63995f8f86539400e48b05bc. Original function declaration/body preserved.
 * The surrounding headers and locale wrappers have been omitted.
 * musl COPYRIGHT (MIT): see ../UPSTREAM-LICENSE.txt.
 */
int isprint(int c)
{
	return (unsigned)c-0x20 < 0x5f;
}
