#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <errno.h>
#include <iconv.h>

#define MAX 1024
static char buf[MAX * 6]; /* UTF-8 needs up to 6 bytes */
static char result[MAX * 6]; /* xml-ification needs up to 6 bytes */

/* The spec says ISO-6937, but many stations get it wrong and use ISO-8859-1. */
char *iso6937_encoding = "ISO6937";

static int encoding_default(char *t, const char **s, const char *d) {
	strncpy(t, iso6937_encoding, 16);
	return 0;
}
static int encoding_fixed(char *t, const char **s, const char *d) {
	strncpy(t, d, 16);
	*s += 1;
	return 0;
}
static int encoding_variable(char *t, const char **s, const char *d) {
	int i = ((unsigned char)(*s)[1] << 8) +  (unsigned char)(*s)[2];
	snprintf(t, 16, d, i);
	*s += 3;
	return 0;
}
static int encoding_reserved(char *t, const char **s, const char *d) {
	fprintf(stderr, "Reserved encoding: %02x\n", *s[0]);
	return 1;
}
static const struct encoding {
	int (*handler)(char *t, const char **s, const char *d);
	const char *data;
} encoding[256] = {
	[0x00] = {encoding_reserved, NULL},
	[0x01] = {encoding_fixed, "ISO-8859-5"},
	[0x02] = {encoding_fixed, "ISO-8859-6"},
	[0x03] = {encoding_fixed, "ISO-8859-7"},
	[0x04] = {encoding_fixed, "ISO-8859-8"},
	[0x05] = {encoding_fixed, "ISO-8859-9"},
	[0x06] = {encoding_fixed, "ISO-8859-10"},
	[0x07] = {encoding_fixed, "ISO-8859-11"},
	[0x08] = {encoding_fixed, "ISO-8859-12"},
	[0x09] = {encoding_fixed, "ISO-8859-13"},
	[0x0A] = {encoding_fixed, "ISO-8859-14"},
	[0x0B] = {encoding_fixed, "ISO-8859-15"},
	[0x0C] = {encoding_reserved, NULL},
	[0x0D] = {encoding_reserved, NULL},
	[0x0E] = {encoding_reserved, NULL},
	[0x0F] = {encoding_reserved, NULL},
	[0x10] = {encoding_variable, "ISO-8859-%d"},
	[0x11] = {encoding_fixed, "ISO-10646/UCS2"}, // FIXME: UCS-2 LE/BE ???
	[0x12] = {encoding_fixed, "KSC_5601"}, // TODO needs newer iconv
	[0x13] = {encoding_fixed, "GB_2312-80"},
	[0x14] = {encoding_fixed, "BIG5"},
	[0x15] = {encoding_fixed, "ISO-10646/UTF8"},
	[0x16] = {encoding_reserved, NULL},
	[0x17] = {encoding_reserved, NULL},
	[0x18] = {encoding_reserved, NULL},
	[0x19] = {encoding_reserved, NULL},
	[0x1A] = {encoding_reserved, NULL},
	[0x1B] = {encoding_reserved, NULL},
	[0x1C] = {encoding_reserved, NULL},
	[0x1D] = {encoding_reserved, NULL},
	[0x1E] = {encoding_reserved, NULL},
	[0x1F] = {encoding_reserved, NULL},
	[0x20 ... 0xFF] = {encoding_default, NULL},
};
static char cs_old[16];
static iconv_t cd;

/* Quote the xml entities in the string passed in.
 */
char *xmlify(const char *s) {
	char cs_new[16];

	int i = (int)(unsigned char)s[0];
	if (encoding[i].handler(cs_new, &s, encoding[i].data))
		return "";
	if (strncmp(cs_old, cs_new, 16)) {
		if (cd) {
			iconv_close(cd);
			cd = NULL;
		} // if
		cd = iconv_open("UTF-8", cs_new);
		if (cd == (iconv_t)-1) {
			fprintf(stderr, "iconv_open() failed: %s\n", strerror(errno));
			exit(1);
		} // if
		strncpy(cs_old, cs_new, 16);
	} // if

	char *inbuf = (char *)s;
	size_t inbytesleft = strlen(s);
	char *outbuf = (char *)buf;
	size_t outbytesleft = sizeof(buf);
	size_t ret = iconv(cd, &inbuf, &inbytesleft, &outbuf, &outbytesleft);
	if (ret == (size_t)-1) {
		fprintf(stderr, "iconv() failed: %s\n", strerror(errno));
		// exit(1); // FIXME: handle errors
	} // if

	// Luckily '&<> are single byte character sequences in UTF-8 and no
	// other character will have a UTF-8 sequence containing these
	// patterns. Because the MSB is set in all multi-byte sequences, we can
	// simply scan for '&<> and don't have to parse UTF-8 sequences.

	char *b = buf, *r = result;
	for ( ; b < outbuf; b++)
		switch (*b) {
#if 0 // only needed for attributes
			case '"':
				*r++ = '&';
				*r++ = 'q';
				*r++ = 'u';
				*r++ = 'o';
				*r++ = 't';
				*r++ = ';';
				break;
#endif
			case '&':
				*r++ = '&';
				*r++ = 'a';
				*r++ = 'm';
				*r++ = 'p';
				*r++ = ';';
				break;
			case '<':
				*r++ = '&';
				*r++ = 'l';
				*r++ = 't';
				*r++ = ';';
				break;
			case '>':
				*r++ = '&';
				*r++ = 'g';
				*r++ = 't';
				*r++ = ';';
				break;
			case 0x0000 ... 0x0008:
			case 0x000B ... 0x001F:
			case 0x007F:
				fprintf(stderr, "Forbidden char %02x\n", *b);
			default:
				*r++ = *b;
				break;
		} // switch

	*r = '\0';
	return result;
} // xmlify

#ifdef MAIN
int main(int argc, char **argv) {
	if (argc > 1)
		printf("%s\n%s\n", argv[1], xmlify(argv[1]));
	return 0;
} // main
#endif
