class TunnelStats {
  final int activeConnections;
  final int totalBytesUp;
  final int totalBytesDown;
  final Duration? lastPingLatency;
  final DateTime? connectedSince;

  const TunnelStats({
    this.activeConnections = 0,
    this.totalBytesUp = 0,
    this.totalBytesDown = 0,
    this.lastPingLatency,
    this.connectedSince,
  });

  Duration get uptime => connectedSince != null
      ? DateTime.now().difference(connectedSince!)
      : Duration.zero;

  TunnelStats copyWith({
    int? activeConnections,
    int? totalBytesUp,
    int? totalBytesDown,
    Duration? lastPingLatency,
    DateTime? connectedSince,
  }) {
    return TunnelStats(
      activeConnections: activeConnections ?? this.activeConnections,
      totalBytesUp: totalBytesUp ?? this.totalBytesUp,
      totalBytesDown: totalBytesDown ?? this.totalBytesDown,
      lastPingLatency: lastPingLatency ?? this.lastPingLatency,
      connectedSince: connectedSince ?? this.connectedSince,
    );
  }

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is TunnelStats &&
          activeConnections == other.activeConnections &&
          totalBytesUp == other.totalBytesUp &&
          totalBytesDown == other.totalBytesDown &&
          lastPingLatency == other.lastPingLatency &&
          connectedSince == other.connectedSince;

  @override
  int get hashCode => Object.hash(
        activeConnections,
        totalBytesUp,
        totalBytesDown,
        lastPingLatency,
        connectedSince,
      );
}
