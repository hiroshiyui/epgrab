#! /usr/bin/awk -f
BEGIN \
{
	print "#include \"tv_grab_dvb.h\""
	print "const struct lookup_table languageid_table[] = {"
}
/^#/ \
{
	next
}
/^$/ \
{
	next
}
$3 == "XX" \
{
	next
}
{
	print "\t{{.c=\""$1"\"}, \""$3"\"},"
}
$1 != $2 \
{
	print "\t{{.c=\""$2"\"}, \""$3"\"},"
}
END \
{
	print "\t{{-1}, NULL},"
	print "};"
}
