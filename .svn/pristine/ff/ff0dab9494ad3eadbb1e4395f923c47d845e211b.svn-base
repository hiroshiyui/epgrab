#ifndef __tv_grab_dvd
#define __tv_grab_dvd

#include <stdint.h>
#include <stdlib.h>

/* lookup.c */
union lookup_key {
	int i;
	char c[4];
};
struct lookup_table {
	union lookup_key u;
	char *desc;
};

extern char *lookup(const struct lookup_table *l, int id);
extern int load_lookup(struct lookup_table **l, const char *file);

/* dvb_info_tables.c */
extern const struct lookup_table description_table[];
extern const struct lookup_table aspect_table[];
extern const struct lookup_table audio_table[];
extern const struct lookup_table crid_type_table[];

/* langidents.c */
extern const struct lookup_table languageid_table[];

/* crc32.c */
extern uint32_t _dvb_crc32(const uint8_t *data, size_t len);

/* dvb_text.c */
extern char *xmlify(const char *s);
extern char *iso6937_encoding;

#endif
