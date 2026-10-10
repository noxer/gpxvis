// Dump the results of the C++ track code (gpx.cpp) as JSON, for comparison
// with the Rust port. Built against the original sources, see compare.py.
#include "gpx.h"

#include <cmath>
#include <cstdio>
#include <vector>

static FILE *out = stdout;

static const double FRACTIONS[] = {-0.1, 0.0, 0.001, 0.1, 0.25, 0.5, 0.75, 0.9, 0.999, 1.0, 1.1};
static const double QUERY_OFFSETS[][2] = {{0.0, 0.0}, {0.01, 0.0}, {0.0, -0.05}, {0.3, 0.2}, {-2.0, 5.0}};

static void num(double v) { std::isfinite(v) ? fprintf(out, "%.17g", v) : fprintf(out, "null"); }

static void list(const std::vector<double>& v)
{
	fprintf(out, "[");
	for (size_t i = 0; i < v.size(); i++) {
		if (i) fprintf(out, ",");
		num(v[i]);
	}
	fprintf(out, "]");
}

static void pause(const gpx::CTrack& t, const gpx::TPauseDetectorConfig& cfg)
{
	double dur, dist;
	size_t cnt = t.GetPauseStats(dur, dist, cfg);
	fprintf(out, "{\"count\":%zu,\"duration\":", cnt);
	num(dur);
	fprintf(out, ",\"distance\":");
	num(dist);
	fprintf(out, "}");
}

int main(int argc, char **argv)
{
	// gpx.cpp logs to stdout, so the JSON goes to the file named by argv[1]
	if (argc < 2 || !(out = fopen(argv[1], "w"))) {
		fprintf(stderr, "usage: %s OUT.json FILE.gpx...\n", argv[0]);
		return 1;
	}
	fprintf(out, "{");
	for (int a = 2; a < argc; a++) {
		gpx::CTrack t;
		bool ok = t.Load(argv[a]);
		fprintf(out, "%s\"%s\":{\"ok\":%s", a > 2 ? "," : "", argv[a], ok ? "true" : "false");
		if (ok) {
			const std::vector<gpx::TPoint>& p = t.GetPoints();
			size_t n = p.size();
			fprintf(out, ",\"count\":%zu,\"length\":", n);
			num(t.GetLength());
			fprintf(out, ",\"duration\":");
			num(t.GetDuration());
			fprintf(out, ",\"start\":%lld,\"aabb\":", (long long)t.GetStartTimestamp());
			const double *bb = t.GetAABB().Get();
			list(std::vector<double>(bb, bb + 6));

			fprintf(out, ",\"points\":[");
			for (size_t i = 0; i < n; i++) {
				if (i) fprintf(out, ",");
				list({p[i].lon, p[i].lat, p[i].x, p[i].y, p[i].h, p[i].len, p[i].duration,
				      p[i].posOnTrack, p[i].timeOnTrack, (double)p[i].timestamp});
			}
			fprintf(out, "]");

			std::vector<double> byDist, byDur;
			for (double f : FRACTIONS) {
				byDist.push_back(t.GetPointByDistance(f * t.GetLength()));
				byDur.push_back(t.GetPointByDuration(f * t.GetDuration()));
			}
			fprintf(out, ",\"by_distance\":"); list(byDist);
			fprintf(out, ",\"by_duration\":"); list(byDur);

			std::vector<double> distAt, durAt;
			for (double f : FRACTIONS) {
				float pos = (float)(f * (double)(n - 1));
				distAt.push_back(t.GetDistanceAt(pos));
				durAt.push_back(t.GetDurationAt(pos));
			}
			fprintf(out, ",\"distance_at\":"); list(distAt);
			fprintf(out, ",\"duration_at\":"); list(durAt);

			std::vector<double> dsq;
			const gpx::TPoint& mid = p[n / 2];
			for (const auto& o : QUERY_OFFSETS) {
				dsq.push_back(t.GetDistanceSqrTo(mid.x + o[0], mid.y + o[1]));
			}
			fprintf(out, ",\"dist_sqr\":"); list(dsq);

			gpx::TPauseDetectorConfig def, tight;
			tight.pauseMaxRadius = 0.010;
			tight.pauseMinDuration = 60.0;
			tight.pauseContinueRadius = 0.050;
			tight.pauseContinueDuration = 60.0;
			fprintf(out, ",\"pause_default\":"); pause(t, def);
			fprintf(out, ",\"pause_tight\":"); pause(t, tight);
		}
		fprintf(out, "}");
	}
	fprintf(out, "}\n");
	fclose(out);
	return 0;
}
