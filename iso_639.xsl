<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet version="1.0"
  xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="text" indent="no" omit-xml-declaration="yes"/>

  <xsl:template match="/">
    <xsl:text>#include "tv_grab_dvb.h"
const struct lookup_table languageid_table[] = {
</xsl:text>
    <xsl:for-each select="/iso_639_entries/iso_639_entry[@iso_639_1_code]">
      <xsl:text>	{{.c="</xsl:text>
      <xsl:value-of select="@iso_639_2B_code"/>
      <xsl:text>"}, "</xsl:text>
      <xsl:value-of select="@iso_639_1_code"/>
      <xsl:text>"},
</xsl:text>
      <xsl:if test="@iso_639_2B_code != @iso_639_2T_code">
        <xsl:text>	{{.c="</xsl:text>
        <xsl:value-of select="@iso_639_2T_code"/>
        <xsl:text>"}, "</xsl:text>
        <xsl:value-of select="@iso_639_1_code"/>
        <xsl:text>"},
</xsl:text>
      </xsl:if>
    </xsl:for-each>
    <xsl:text>	{{-1}, NULL},
};
</xsl:text>
  </xsl:template>

</xsl:stylesheet>
<!-- vim: autoindent smartindent tabstop=2 shiftwidth=2 expandtab
-->
