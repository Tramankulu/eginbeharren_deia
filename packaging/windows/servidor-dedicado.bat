@echo off
title Servidor dedicado de Eginbeharraren deia
rem Cambia las opciones a tu gusto. Ejecuta "eginbeharraren-deia-server.exe --ayuda" para verlas todas.
"%~dp0eginbeharraren-deia-server.exe" --nombre "Servidor de Eginbeharraren deia" --max 16 --modos tdm,dm,sd,ctf
pause
