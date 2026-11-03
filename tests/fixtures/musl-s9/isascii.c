/* Extract from ifduyue/musl src/ctype/isascii.c at
 * commit 7813e9119eb5fe681b6750b8a033a0490bca0941
 * upstream blob 54ad3bf028a6728102e2e311284fece49f2eac71. Original function declaration/body preserved.
 * The surrounding headers and locale wrappers have been omitted.
 * musl COPYRIGHT (MIT): see ../UPSTREAM-LICENSE.txt.
 */
int isascii(int c)
{
	return !(c&~0x7f);
}
