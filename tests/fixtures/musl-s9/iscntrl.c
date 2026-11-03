/* Extract from ifduyue/musl src/ctype/iscntrl.c at
 * commit 7813e9119eb5fe681b6750b8a033a0490bca0941
 * upstream blob f27837ea53c8cc732fd1478764e351c5569b4888. Original function declaration/body preserved.
 * The surrounding headers and locale wrappers have been omitted.
 * musl COPYRIGHT (MIT): see ../UPSTREAM-LICENSE.txt.
 */
int iscntrl(int c)
{
	return (unsigned)c < 0x20 || c == 0x7f;
}
