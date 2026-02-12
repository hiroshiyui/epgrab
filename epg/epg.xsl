<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="html" encoding="UTF-8" />

  <xsl:template name="format-time">
    <xsl:param name="dt" />
    <!-- Input: YYYYMMDDHHmmSS +HHMM -->
    <xsl:value-of select="substring($dt,1,4)" />-<xsl:value-of select="substring($dt,5,2)" />-<xsl:value-of select="substring($dt,7,2)" />
    <xsl:text> </xsl:text>
    <xsl:value-of select="substring($dt,9,2)" />:<xsl:value-of select="substring($dt,11,2)" />
  </xsl:template>

  <xsl:template match="/tv">
    <html lang="en">
      <head>
        <meta charset="UTF-8" />
        <title><xsl:value-of select="channel/display-name" /> - EPG</title>
        <style>
          body {
            font-family: system-ui, -apple-system, sans-serif;
            max-width: 960px;
            margin: 2rem auto;
            padding: 0 1rem;
            color: #1a1a1a;
            background: #fafafa;
          }
          h1 {
            border-bottom: 2px solid #333;
            padding-bottom: 0.5rem;
          }
          .meta {
            color: #666;
            font-size: 0.9rem;
            margin-bottom: 1.5rem;
          }
          table {
            width: 100%;
            border-collapse: collapse;
          }
          th {
            background: #333;
            color: #fff;
            text-align: left;
            padding: 0.6rem 0.8rem;
          }
          td {
            padding: 0.5rem 0.8rem;
            border-bottom: 1px solid #ddd;
            vertical-align: top;
          }
          tr:hover td {
            background: #f0f0f0;
          }
          .time {
            white-space: nowrap;
            font-variant-numeric: tabular-nums;
          }
          .title {
            font-weight: 600;
          }
          .desc {
            color: #444;
            font-size: 0.9rem;
          }
        </style>
      </head>
      <body>
        <h1><xsl:value-of select="channel/display-name" /></h1>
        <p class="meta">
          <xsl:value-of select="count(programme)" /> programmes
        </p>
        <table>
          <thead>
            <tr>
              <th>Start</th>
              <th>End</th>
              <th>Programme</th>
            </tr>
          </thead>
          <tbody>
            <xsl:for-each select="programme">
              <tr>
                <td class="time">
                  <xsl:call-template name="format-time">
                    <xsl:with-param name="dt" select="@start" />
                  </xsl:call-template>
                </td>
                <td class="time">
                  <xsl:call-template name="format-time">
                    <xsl:with-param name="dt" select="@stop" />
                  </xsl:call-template>
                </td>
                <td>
                  <div class="title"><xsl:value-of select="title" /></div>
                  <xsl:if test="desc">
                    <div class="desc"><xsl:value-of select="desc" /></div>
                  </xsl:if>
                </td>
              </tr>
            </xsl:for-each>
          </tbody>
        </table>
      </body>
    </html>
  </xsl:template>
</xsl:stylesheet>
