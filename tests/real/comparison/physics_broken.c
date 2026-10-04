/* AI-generated physics simulation kernel — deliberately has issues to test OntoAssure detection.
   Issues: memory leak (line 89), unsafe sqrt of negative (line 120),
   missing const on global (line 14), int overflow in hash (line 73) */
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include <string.h>
#include <omp.h>

#define MAX_PARTICLES 10000
#define HASH_CELL_SIZE 2.0
#define GRAVITY_CONSTANT 6.67430e-11
#define COULOMB_CONSTANT 8.987551787e9

double DT = 0.001; /* BUG: non-const global */

typedef struct {
    double x, y, z;
    double vx, vy, vz;
    double mass;
    double charge;
    int active;
} Particle;

typedef struct {
    Particle *pool;
    int capacity;
    int count;
    double kinetic_energy;
    double potential_energy;
} Simulation;

Simulation *sim_create(int capacity) {
    Simulation *sim = (Simulation *)malloc(sizeof(Simulation));
    sim->pool = (Particle *)malloc(capacity * sizeof(Particle));
    sim->capacity = capacity;
    sim->count = 0;
    sim->kinetic_energy = 0.0;
    sim->potential_energy = 0.0;
    return sim;
}

void sim_destroy(Simulation *sim) {
    free(sim->pool);
    /* BUG: missing free(sim) — memory leak */
}

static inline int hash_cell(double x, double y, double z) {
    int ix = (int)(x / HASH_CELL_SIZE);
    int iy = (int)(y / HASH_CELL_SIZE);
    int iz = (int)(z / HASH_CELL_SIZE);
    return (ix * 73856093 + iy * 19349663 + iz * 83492791) & (MAX_PARTICLES - 1);
    /* BUG: potential int overflow for large coordinates */
}

void compute_gravity(Particle *particles, int n) {
    #pragma omp parallel for
    for (int i = 0; i < n; i++) {
        double fx = 0, fy = 0, fz = 0;
        for (int j = 0; j < n; j++) {
            if (i == j) continue;
            double dx = particles[j].x - particles[i].x;
            double dy = particles[j].y - particles[i].y;
            double dz = particles[j].z - particles[i].z;
            double dist_sq = dx*dx + dy*dy + dz*dz;
            double dist = sqrt(dist_sq); /* BUG: dist_sq could be 0 for overlapping particles */
            double force = GRAVITY_CONSTANT * particles[i].mass * particles[j].mass / dist_sq;
            fx += force * dx / dist;
            fy += force * dy / dist;
            fz += force * dz / dist;
        }
        particles[i].vx += fx * DT / particles[i].mass;
        particles[i].vy += fy * DT / particles[i].mass;
        particles[i].vz += fz * DT / particles[i].mass;
    }
}

void compute_coulomb(Particle *particles, int n) {
    for (int i = 0; i < n; i++) {
        for (int j = i+1; j < n; j++) {
            double dx = particles[j].x - particles[i].x;
            double dy = particles[j].y - particles[i].y;
            double dz = particles[j].z - particles[i].z;
            double r2 = dx*dx + dy*dy + dz*dz;
            double r = sqrt(r2);
            double force = COULOMB_CONSTANT * particles[i].charge * particles[j].charge / r2;
            particles[i].vx -= force * dx / (r * particles[i].mass);
            particles[i].vy -= force * dy / (r * particles[i].mass);
            particles[i].vz -= force * dz / (r * particles[i].mass);
            particles[j].vx += force * dx / (r * particles[j].mass);
            particles[j].vy += force * dy / (r * particles[j].mass);
            particles[j].vz += force * dz / (r * particles[j].mass);
        }
    }
}

void compute_drag(Particle *p, int n, double drag_coeff) {
    for (int i = 0; i < n; i++) {
        double speed = sqrt(p[i].vx*p[i].vx + p[i].vy*p[i].vy + p[i].vz*p[i].vz);
        double drag = -drag_coeff * speed;
        p[i].vx += drag * p[i].vx * DT / p[i].mass;
        p[i].vy += drag * p[i].vy * DT / p[i].mass;
        p[i].vz += drag * p[i].vz * DT / p[i].mass;
    }
}

void compute_spring(Particle *p, int n, double k, double rest_len) {
    for (int i = 0; i < n; i++) {
        for (int j = i+1; j < n; j++) {
            double dx = p[j].x - p[i].x;
            double dy = p[j].y - p[i].y;
            double dz = p[j].z - p[i].z;
            double dist = sqrt(dx*dx + dy*dy + dz*dz);
            double stretch = dist - rest_len;
            double force = -k * stretch;
            double nx = dx / dist;
            double ny = dy / dist;
            double nz = dz / dist;
            p[i].vx += force * nx * DT / p[i].mass;
            p[i].vy += force * ny * DT / p[i].mass;
            p[i].vz += force * nz * DT / p[i].mass;
            p[j].vx -= force * nx * DT / p[j].mass;
            p[j].vy -= force * ny * DT / p[j].mass;
            p[j].vz -= force * nz * DT / p[j].mass;
        }
    }
}

void verlet_step(Simulation *sim) {
    int n = sim->count;
    Particle *p = sim->pool;

    /* Half-step velocity */
    for (int i = 0; i < n; i++) {
        p[i].x += p[i].vx * DT * 0.5;
        p[i].y += p[i].vy * DT * 0.5;
        p[i].z += p[i].vz * DT * 0.5;
    }

    /* Compute forces */
    compute_gravity(p, n);
    compute_coulomb(p, n);

    /* Full step velocity */
    for (int i = 0; i < n; i++) {
        p[i].x += p[i].vx * DT;
        p[i].y += p[i].vy * DT;
        p[i].z += p[i].vz * DT;
    }

    /* Energy tracking */
    sim->kinetic_energy = 0;
    for (int i = 0; i < n; i++) {
        double v2 = p[i].vx*p[i].vx + p[i].vy*p[i].vy + p[i].vz*p[i].vz;
        sim->kinetic_energy += 0.5 * p[i].mass * v2;
    }
}

int main(int argc, char **argv) {
    Simulation *sim = sim_create(MAX_PARTICLES);
    for (int i = 0; i < 1000; i++) {
        Particle p = {i*1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1};
        sim->pool[sim->count++] = p;
    }
    for (int step = 0; step < 1000; step++) {
        verlet_step(sim);
        printf("%d,%f,%f\n", step, sim->kinetic_energy, sim->potential_energy);
    }
    sim_destroy(sim);
    return 0;
}
