/* Extract from ifduyue/musl src/ctype/toascii.c at
 * commit 7813e9119eb5fe681b6750b8a033a0490bca0941
 * upstream blob f0e48e8e91761585db15fbc6a6f2e5fa74496c23. Original function declaration/body preserved.
 * The surrounding headers and locale wrappers have been omitted.
 * musl COPYRIGHT (MIT): see ../UPSTREAM-LICENSE.txt.
 */
int toascii(int c)
{
	return c & 0x7f;
}
