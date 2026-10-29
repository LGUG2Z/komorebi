#!/usr/bin/env perl
# Type a string into a QEMU guest through QMP send-key, for the install
# probes before the agent exists. usage: qmp-type.pl SOCKET TEXT [enter]
use strict;
use warnings;
use IO::Socket::UNIX;

my ($sock, $text, $enter) = @ARGV;
my %sym = (
  ' ' => 'spc', '\\' => 'backslash', '/' => 'slash', '.' => 'dot',
  ',' => 'comma', '-' => 'minus', '=' => 'equal', ';' => 'semicolon',
  "'" => 'apostrophe', '[' => 'bracket_left', ']' => 'bracket_right',
  '`' => 'grave_accent',
);
my %shifted = (
  ':' => 'semicolon', '_' => 'minus', '+' => 'equal', '"' => 'apostrophe',
  '|' => 'backslash', '<' => 'comma', '>' => 'dot', '?' => 'slash',
  '{' => 'bracket_left', '}' => 'bracket_right', '~' => 'grave_accent',
  '!' => '1', '@' => '2', '#' => '3', '$' => '4', '%' => '5', '^' => '6',
  '&' => '7', '*' => '8', '(' => '9', ')' => '0',
);

my $s = IO::Socket::UNIX->new(Peer => $sock) or die "connect $sock: $!";
<$s>;
sub cmd {
  my ($c) = @_;
  print $s "$c\n";
  while (my $l = <$s>) { last if $l =~ /"return"|"error"/ }
}
cmd('{"execute":"qmp_capabilities"}');

sub keys_json { join ',', map { qq({"type":"qcode","data":"$_"}) } @_ }
for my $c (split //, $text) {
  my @k;
  if ($c =~ /[a-z0-9]/) { @k = ($c) }
  elsif ($c =~ /[A-Z]/) { @k = ('shift', lc $c) }
  elsif (exists $sym{$c}) { @k = ($sym{$c}) }
  elsif (exists $shifted{$c}) { @k = ('shift', $shifted{$c}) }
  else { die "no key for '$c'" }
  cmd('{"execute":"send-key","arguments":{"keys":[' . keys_json(@k) . ']}}');
  select(undef, undef, undef, 0.05);
}
cmd('{"execute":"send-key","arguments":{"keys":[' . keys_json('ret') . ']}}')
  if $enter;
